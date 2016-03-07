use std::fmt::{self,Formatter};
use std::cell::{RefCell,RefMut,Ref};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::marker;

use nalgebra::{Pnt2,Vec2};
use rand;

use id::{Id,HasForgeableId};
use data::{Map,Player,Stats,Position};
use messages::{EntityState};
use self::hitbox::RectangleHitbox;

mod status;
mod update;
mod hitbox;
//mod serialize;

pub use self::update::update;
pub use lycan_serialize::Order;

pub use lycan_serialize::Direction;

static DEFAULT_SPEED:    f32 = 10.0;
static DEFAULT_AI_SPEED: f32 = 5.0;

#[derive(Debug)]
pub struct Entity {
    id: Id<Entity>,

    e_type: EntityType,
    position: Pnt2<f32>,
    // We probably won't save the speed ...
    speed: Vec2<f32>,
    orientation: Direction,
    skin: u64,
    pv: u64,
    hitbox: RectangleHitbox,
    attack_box: RectangleHitbox,
    attack_offset_x: Vec2<f32>,
    attack_offset_y: Vec2<f32>,
    base_stats: Stats,
    stats: CurrentStats,

    // TODO: Replace by a FSM
    walking: bool,
    attacking: u64, // XXX: This is currently expressed in tick, not ms!
}

lazy_static! {
    static ref NEXT_SKIN: AtomicUsize = AtomicUsize::new(0);
}

impl Entity {
    pub fn new(e_type: EntityType,
               position: Pnt2<f32>,
               orientation: Direction,
               skin: u64,
               base_stats: Stats,
               pv: u64,
               )
        -> Entity {
            let mut e = Entity {
                id: Id::new(),

                e_type: e_type,
                position: position,
                speed: Vec2::new(0.0,0.0),
                orientation: orientation,
                base_stats: base_stats,
                stats: Default::default(),
                skin: skin,
                pv: pv,
                hitbox: RectangleHitbox::new_default(),
                attack_box: RectangleHitbox::new(0.5, 0.5),
                attack_offset_x: Vec2::new(0.75, 0.0),
                attack_offset_y: Vec2::new(0.0, 1.0),

                walking: false,
                attacking: 0,
            };
            e.recompute_current_stats();
            e
        }

    pub fn get_id(&self) -> Id<Self> {
        self.id
    }

    pub fn get_position(&self) -> Pnt2<f32> {
        self.position
    }

    pub fn get_skin(&self) -> u64 {
        self.skin
    }

    pub fn get_pv(&self) -> u64 {
        self.pv
    }

    pub fn get_orientation(&self) -> Direction {
        self.orientation
    }

    pub fn get_type(&self) -> &EntityType {
        &self.e_type
    }

    pub fn recompute_current_stats(&mut self) {
        let speed = match self.e_type {
            EntityType::Player(_) => DEFAULT_SPEED,
            EntityType::Invoked(_) => DEFAULT_AI_SPEED,
        };
        self.stats.speed = speed;
    }

    pub fn walk(&mut self, orientation: Option<Direction>) {
        match orientation {
            Some(o) => {
                self.walking = true;
                self.orientation = o;
            }
            None => {
                self.walking = false;
            }
        }
    }

    // TODO: Remove
    pub fn fake_player(id: Id<Player>) -> Entity {
        let stats = Stats {
            level:          1,
            strength:       2,
            dexterity:      3,
            constitution:   4,
            intelligence:   5,
            precision:      6,
            wisdom:         7,
        };
        let position = Position {
            x: 0.0,
            y: 0.0,
            map: Id::forge(1)
        };
        let name =  match id.as_u64() {
            0 => {
                "Vaelden".to_owned()
            }
            1 => {
                "Cendrais".to_owned()
            }
            2 => {
                "Nemikolh".to_owned()
            }
            _ => {
                format!("Player{}", id)
            }
        };
        let skin = NEXT_SKIN.fetch_add(1, Ordering::Relaxed) as u64;
        let player = Player {
            id:         id,
            name:       name,
            skin:       skin,
            current_pv: 100,
            position:   position,
            experience: 0,
            gold:       0,
            guild:      String::new(),
            stats:      stats,
        };
        Entity::from(player)
    }

    pub fn fake_ai() -> Entity {
        let stats = Stats {
            level:          1,
            strength:       2,
            dexterity:      3,
            constitution:   4,
            intelligence:   5,
            precision:      6,
            wisdom:         7,
        };
        let skin = NEXT_SKIN.fetch_add(1, Ordering::Relaxed) as u64;
        Entity::new(
            EntityType::Invoked(None),
            Pnt2::new(1.0, 1.0),
            Direction::South,
            skin,
            stats,
            100)
    }

    pub fn to_entity_state(&self) -> EntityState {
        EntityState::new(self.id, self.position, self.orientation)
    }

    pub fn get_map_position(&self) -> Option<Id<Map>> {
        match self.e_type {
            EntityType::Player(ref player) => Some(player.map),
            _ => None,
        }
    }

    pub fn dump(&self, f: &mut Formatter, indent: &str) -> Result<(),fmt::Error> {
        try!(writeln!(f, "{}Entity {}", indent, self.id));
        match self.e_type {
            EntityType::Player(ref player) => {
                try!(writeln!(f, "{}Player {} {} attached to map {}",
                              indent,
                              player.id,
                              &player.name,
                              player.map));
            }
            EntityType::Invoked(ref parent) => {
                match *parent {
                    Some(parent) => {
                        try!(writeln!(f, "{}Invoked entity attached to {}", indent, parent));
                    }
                    None => {
                        try!(writeln!(f, "{}Invoked entity", indent));
                    }
                }
            }
        }
        // TODO: Presence ...
        try!(writeln!(f, "{}{:?} {:?} {:?}", indent, self.position, self.speed,self.orientation));
        writeln!(f, "{}PV: {}", indent, self.pv)
    }
}

#[derive(Debug)]
pub enum EntityType {
    // An entity can be a player
    Player(PlayerData),
    // Or invoked, with an optional parent
    // XXX: Is the parent really useful?
    Invoked(Option<u64>),
}

// Abstraction so that if we change the implementation it doesn't affect the rest
#[derive(Debug)]
pub struct EntityStore {
    entities: Vec<Entity>,
}

impl EntityStore {
    pub fn new() -> EntityStore {
        EntityStore {
            entities: Vec::new(),
        }
    }

    pub fn push(&mut self, entity: Entity) {
        self.entities.push(entity)
    }

    pub fn remove(&mut self, id: Id<Entity>) -> Option<Entity> {
        let position = match self.get_position(id) {
            Some(pos) => pos,
            None => return None,
        };

        Some(self.entities.remove(position))
    }

    pub fn get(&self, id: Id<Entity>) -> Option<&Entity> {
        self.get_position(id).map(move |position| self.entities.get(position).unwrap())
    }

    pub fn get_mut(&mut self, id: Id<Entity>) -> Option<&mut Entity> {
        self.get_position(id).map(move |position| self.entities.get_mut(position).unwrap())
    }

    pub fn get_mut_wrapper<'a>(&'a mut self, id: Id<Entity>) -> Option<(&'a mut Entity, Wrapper<'a>)> {
        self.get_position(id).map(move |position| {
            Wrapper::new(&mut self.entities, position).unwrap()
        })
    }

    fn get_position(&self, id: Id<Entity>) -> Option<usize> {
        for (position, entity) in self.entities.iter().enumerate() {
            if entity.get_id() == id {
                return Some(position);
            }
        }
        None
    }

    pub fn iter(&self) -> ::std::slice::Iter<Entity> {
        self.entities.iter()
    }

    pub fn iter_mut(&mut self) -> ::std::slice::IterMut<Entity> {
        self.entities.iter_mut()
    }

    pub fn iter_mut_wrapper(&mut self) -> IterMutWrapper {
        IterMutWrapper {
            inner: &mut self.entities,
            current_position: 0,
        }
    }
}

pub struct Wrapper<'a> {
    inner: &'a mut [Entity],
    borrowed_entity_position: usize,
}

impl <'a> Wrapper<'a> {
    pub fn new(a: &'a mut [Entity], position: usize) -> Option<(&'a mut Entity, Wrapper<'a>)> {
        let entity: &mut Entity = unsafe {
            match a.get_mut(position) {
                None => return None,
                Some(entity) => ::std::mem::transmute(entity),
            }
        };
        let wrapper = Wrapper {
            inner: a,
            borrowed_entity_position: position
        };
        Some((entity, wrapper))
    }

    pub fn get_by_index(&mut self, index: usize) -> Option<&mut Entity> {
        if index == self.borrowed_entity_position {
            None
        } else {
            self.inner.get_mut(index)
        }
        /*
        let entity = self.inner.get(
        let a: &mut [T] = unsafe { mem::transmute(self.inner as *mut [T]) };
        a.get_mut(index)
        */
    }

    pub fn get(&mut self, id: Id<Entity>) -> Option<&mut Entity> {
        match self.get_position(id) {
            Some(pos) => self.get_by_index(pos),
            None => None,
        }
    }

    pub fn iter(&mut self) -> WrapperIter {
        let p = self.inner.as_mut_ptr();
        unsafe {
            WrapperIter {
                ptr: p,
                end: p.offset(self.inner.len() as isize) ,
                borrowed_entity: p.offset(self.borrowed_entity_position as isize),
                _marker: marker::PhantomData,
            }
        }
    }

    // XXX: We should probably have a &self version
    pub fn get_position(&mut self, id: Id<Entity>) -> Option<usize> {
        let borrowed = self.borrowed_entity_position;
        for (position, entity) in self.iter().enumerate() {
            if entity.get_id() == id {
                let adjusted_position = if position >= borrowed {
                    position + 1
                } else {
                    position
                };
                return Some(adjusted_position);
            }
        }
        None
    }
}

// TODO: Have a *const version
pub struct WrapperIter<'a> {
    ptr: *mut Entity,
    end: *mut Entity,
    borrowed_entity: *mut Entity,
    _marker: marker::PhantomData<&'a mut Entity>,
}

impl <'a> Iterator for WrapperIter<'a> {
    type Item = &'a mut Entity;

    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        if self.ptr == self.end {
            None
        } else {
            let old = self.ptr;
            self.ptr = unsafe { self.ptr.offset(1) };
            if old == self.borrowed_entity {
                self.next()
            } else {
                unsafe { Some(::std::mem::transmute(old)) }
            }
        }
    }
}

pub struct IterMutWrapper<'a> {
    inner: &'a mut [Entity],
    current_position: usize,
}

// Cannot implement Iterator because an item borrows the iterator
impl <'a> IterMutWrapper<'a> {
    pub fn next_item<'b>(&'b mut self) -> Option<(&'b mut Entity, Wrapper<'b>)> {
        let res = Wrapper::new(self.inner, self.current_position);
        self.current_position += 1;
        res
    }
}

#[cfg(test)]
mod test {
    use super::{Entity, EntityStore};
    use id::Id;
    #[test]
    fn test() {
        let mut store = EntityStore::new();
        store.push(Entity::fake_player(Id::forge(0)));
        store.push(Entity::fake_player(Id::forge(1)));
        store.push(Entity::fake_player(Id::forge(2)));
        {
            let mut double_iter = store.iter_mut_wrapper();
            while let Some((entity,mut wrapper)) = double_iter.next_item() {
                let id = entity.get_id();
                for other in wrapper.iter() {
                    assert!(id != other.get_id());
                }
                assert!(wrapper.get(id).is_none());
            }
        }
    }
}

#[derive(Debug,Clone,Default)]
struct CurrentStats {
    speed: f32,
}

#[derive(Debug,Clone)]
pub struct PlayerData {
    name: String,
    id: Id<Player>,
    map: Id<Map>,
}

impl PlayerData {
    pub fn get_id(&self) -> Id<Player> {
        self.id
    }
}

impl From<Player> for Entity {
    fn from(player: Player) -> Entity {
        let mut entity = Entity::new(
            EntityType::Player(PlayerData {
                name: player.name,
                id: player.id,
                map: player.position.map,
            }),
            Pnt2::new(player.position.x, player.position.y),
            Direction::East,   // TODO
            player.skin,
            player.stats,
            player.current_pv,
            );
        entity.recompute_current_stats();
        entity
    }
}

impl Into<Option<Player>> for Entity {
    fn into(self) -> Option<Player> {
        let player_data = match self.e_type {
            EntityType::Player(player) => player,
            _ => return None,
        };
        let position = Position {
            x: self.position.x,
            y: self.position.y,
            map: player_data.map,
        };
        let player = Player {
            id: player_data.id,
            name: player_data.name,
            skin: self.skin,
            current_pv: self.pv,
            position: position,
            experience: 0,
            gold: 0,
            guild: String::new(),
            stats: self.base_stats,
        };

        Some(player)
    }
}
