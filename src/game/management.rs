use std::thread;
use std::sync::mpsc::{self,Sender};

use mio::Sender as MioSender;
use serde_json::ser::to_string_pretty;
use iron::prelude::*;
use iron::status::Status;
use bodyparser::Struct;
use router::{Router};
use plugin::Extensible;

use lycan_serialize::AuthenticationToken;

use id::{Id,WeakId};
use messages::Request as LycanRequest;
use messages::Command;
use data::{ConnectCharacterParam,AuthenticatedRequest,Map};
use entity::Entity;
use instance::management::*;

// TODO
// - Set correct headers in all responses
// - Check if correct heahers are set (e.g. Content-Type)
// - Authentication of each request
// - Do proper error handling

macro_rules! itry_map {
    ($result:expr, |$err:ident| $bl:expr) => {
        match $result {
            ::std::result::Result::Ok(val) => val,
            ::std::result::Result::Err($err) => {
                return Ok(::iron::response::Response::with($bl));
            }
        }
    };
}

pub fn start_management_api(sender: MioSender<LycanRequest>) {
    thread::spawn(move || {
        let router = create_router(sender);
        let iron = Iron::new(router);

        iron.http("127.0.0.1:8001").unwrap();
    });
}

/// Macro to reduce the boilerplate of creating a channel, create a request, send it to Game and
/// wait for the response
macro_rules! define_request {
    ($sender:ident, |$game:ident, $event_loop:ident| $bl:block) => {{
        let (tx, rx) = mpsc::channel();
        let request = LycanRequest::new(move |$game, $event_loop| {
            let result = $bl;
            let _ = tx.send(result);
        });
        $sender.send(request).unwrap();
        rx.recv().unwrap()
    }};
    ($sender:ident, |$game:ident| $bl:block) => {
        define_request!($sender, |$game, _event_loop| $bl)
    };
}

/// Macro to reduce the boilerplate of creating a channel, create a request, send it to Game
/// Route it to the correct Instance and wait for the response
macro_rules! define_request_instance {
    ($sender:ident, $id:ident, |$instance:ident, $event_loop:ident| $bl:block) => {{
        let (tx, rx) = mpsc::channel();
        let request = LycanRequest::new(move |g, _el| {
            let instance = match g.instances.get(&$id) {
                Some(i) => i,
                None => { let _ = tx.send(Err(())); return; }
            };
            let command = Command::new(move |$instance, $event_loop| {
                let result = $bl;
                let _ = tx.send(Ok(result));
            });
            let _ = instance.send(command);
        });
        $sender.send(request).unwrap();
        rx.recv().unwrap()
    }};
    ($sender:ident, $id:ident, |$instance:ident| $bl:block) => {
        define_request_instance!($sender, $id, |$instance, _event_loop| $bl)
    };
}

// The Rust typechecker doesn't seem to get the types of the closures right
// It infers that they implement FnOnce(...), and therefore do not implement Handler
// This function forces the type of the closure
fn correct_bounds<F>(f: F) -> F
where F: Send + Sync + 'static + Fn(&mut Request) -> IronResult<Response>
{f}

fn create_router(sender: MioSender<LycanRequest>) -> Router {
    let mut server = Router::new();
    // TODO: Add middleware at the beginning for authentication of requests

    let clone = sender.clone();
    server.get("/maps", correct_bounds(move |_request| {
        let maps = define_request!(clone, |game| {
            game.resource_manager.get_all_maps()
        });
        let json = to_string_pretty(&maps).unwrap();
        Ok(Response::with((Status::Ok,json)))
    }));

    let clone = sender.clone();
    server.get("/maps/:id/instances", correct_bounds(move |request| {
        let params = request.extensions.get::<Router>().unwrap();
        // id is part of the route, the unwrap should never fail
        let id = &params["id"];
        let parsed = itry_map!(id.parse::<u64>(), |e| (Status::BadRequest, format!("ERROR: invalid id {}: {}", id, e)));
        let instances = define_request!(clone, |game| {
            game.get_instances(Id::forge(parsed))
        });
        let json = to_string_pretty(&instances).unwrap();
        Ok(Response::with((Status::Ok,json)))
    }));

    let clone = sender.clone();
    server.get("/instances/:id/entities", correct_bounds(move |request| {
        // id is part of the route, the unwrap should never fail
        let params = request.extensions.get::<Router>().unwrap();
        let id = &params["id"];
        let parsed = itry_map!(id.parse::<u64>(), |e| (Status::BadRequest, format!("ERROR: invalid id {}: {}", id, e)));
        let entities = itry_map!(define_request_instance!(clone, parsed, |instance| {
            instance.get_entities()
            }),
            |_e| (Status::BadRequest, format!("ERROR: Non existent instance id {}", parsed)));
        let json = to_string_pretty(&entities).unwrap();
        Ok(Response::with((Status::Ok,json)))
    }));

    let clone = sender.clone();

    server.post("/instances/:id/spawn", correct_bounds(move |request| {
        use data::SpawnMonster;
        let (id_parsed, parsed_monster);

        {
            let params = request.extensions.get::<Router>().unwrap();
            // id is part of the route, the unwrap should never fail
            let id = &params["id"];
            id_parsed = itry_map!(id.parse::<u64>(), |e|
                                  (Status::BadRequest, format!("ERROR: invalid id {}: {}", id, e)));
        }
        {
            let maybe_monster = itry_map!(request.get::<Struct<SpawnMonster>>(), |e|
                                          (Status::BadRequest, format!("ERROR: JSON decoding error: {}", e)));
            parsed_monster = iexpect!(maybe_monster, (Status::BadRequest, "ERROR: No JSON body provided"));
        }
        let monster = itry_map!(
            define_request_instance!(clone, id_parsed, |instance| {
                instance.spawn_monster(parsed_monster)
            }),
            |_e| (Status::BadRequest, format!("ERROR: Non existent instance id {}", id_parsed)));
        let json = to_string_pretty(&monster).unwrap();
        Ok(Response::with((Status::Ok,json)))
    }));

    let clone = sender.clone();
    server.post("/shutdown", correct_bounds(move |_request| {
        define_request!(clone, |g, el| {
            g.start_shutdown(el);
        });
        Ok(Response::with((Status::Ok, "OK")))
    }));

    let clone = sender.clone();
    server.post("/connect_character", correct_bounds(move |request| {
        let maybe_params = itry_map!(request.get::<Struct<AuthenticatedRequest<ConnectCharacterParam>>>(), |e|
                                     (Status::BadRequest, format!("ERROR: JSON decoding error: {}", e)));
        let decoded = iexpect!(maybe_params, (Status::BadRequest, "ERROR: No JSON body provided"));
        debug!("Received request to /connect_character: {:?}", decoded);
        define_request!(clone, |game| {
            let id = Id::forge(decoded.params.id);
            let token = AuthenticationToken(decoded.params.token);
            game.connect_character(id, token);
        });
        Ok(Response::with((Status::Ok, "OK")))
    }));

    let clone = sender.clone();
    fn entity_delete(sender: &MioSender<LycanRequest>, request: &mut Request) -> Result<(),String> {
        let params = request.extensions.get::<Router>().unwrap();
        // id is part of the route, the unwrap should never fail
        let instance_id = {
            let id = &params["instance_id"];
            try!(id.parse::<u64>().map_err(|e| format!("ERROR: invalid instance id {}: {}", id, e)))
        };
        let entity_id: WeakId<Entity> = {
            let id = &params["entity_id"];
            try!(id.parse::<u64>().map_err(|e| format!("ERROR: invalid entity id {}: {}", id, e))).into()
        };
        let result = try!(define_request_instance!(sender, instance_id, |instance| {
            instance.remove_entity(entity_id)
        }).map_err(|_e| format!("ERROR: Non existent instance id {}", instance_id)));
        result.map_err(|e| {
            match e {
                RemoveEntityError::NotFound => format!("ERROR: Entity {} not found in instance {}", entity_id, instance_id),
                RemoveEntityError::IsPlayer => format!("ERROR: Entity {} is a player", entity_id),
            }
        })
    }
    server.delete("/instances/:instance_id/entities/:entity_id", correct_bounds(move |request| {
        match entity_delete(&clone, request) {
            Ok(()) => Ok(Response::with((Status::Ok,"OK"))),
            Err(s) => Ok(Response::with((Status::BadRequest, s))),
        }
    }));

    server
}
