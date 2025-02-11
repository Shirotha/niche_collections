// # Spec
// - either with dealloc or only full clear/cycle
//   TODO: does a freelist add overhead if nothing is individually freed? (how handle ports?)
//   -> dealloc + versioning means arena has to store version data
//     TODO: is versioning needed if handles don't implement Clone (is this feasable?)
//     -> without Clone there can't be multple handles to the same data and references would be unwieldy
//     -> either versioning or clonable handles
//   -> even without versioning dealloc means additional state and minimum size (at least as large as freelist next pointer)
//     TODO: is a freelist even viable for mixed types? (use interval tree instead?)
// - either single type or mixed types (store slices by prefixing them by length or using wide handles)
//   TODO: is there an advantage from having a single type arena when handles are already typed? (alignment?)
// - option for versioning (only needed when dealloc is allowed)
//
// - use branded lifetimes to validate handles
//
// - concurrent access via perpendicular ports
// - three state lock
//   - read: read lock on arena, read lock on port
//   - write: read lock on arena, write lock on port
//   - alloc: upgradable lock on arena, write lock on port
//
// # Types
// ## Arena
// - stores backing Vec
// - carries lifetime guard
// - interacts with Handles with matching lifetime id
// ### Single Type
// - use free-list to reuse memory
// - optional versioning by using union over data and next free pointer
// ### Mixed Types
// TODO: using a free-list can fragmentate memory (use interval tree?)
// - can store unsized data by storing the length in buffer (see: ThinVec)
//
// ## Port
// - view into Arena
// - carries backing arena lifetime id
// - carries lifetime guard
// - can create guarded read/write/alloc access into sections of Arena (guarantied to not alias other Ports)
//
// ## Handle
// - index into Arena/Port
// - carries lifetime id of Arena/Port
// ### Exclusive
// - can't be cloned
// - does not need to carry version information
// ### Shared
// - can be copied
// - need to carry version information
//
// # Implementation
// - single Arena type using generics
// - different traits for different strategies
//   - single type, mixed types as two traits (mixed types trait also implements single type)
//     -> TypedStore
//       - insert based on generic type param
//     -> MixedStore: TypedStore (with default impl)
//       - insert based on generic function param
//       NOTE: needs to control backing buffer (type can't be assumed to be T as in TypedStore)
//       NOTE: Arena should be able to use shared impl where possible
//       NOTE: buffer type has to be statically known and be at the same location for both TypedStore and MixedStore
//   - reuse strategy: none (no trait impl), free-list, interval map (?), ... as different impls of reuse trait
//     -> ReclaimMemory trait (optional: not impl this just disables dealloc support)
//       - possibly needs its own fields and may change type stored in data buffer (e.g. T -> T or nextptr)
//       -> associated type Wrapper<T>? (= T if unused)
//   - exclusive handles vs versioning vs unsafe access
//     TODO: their should be a single trait that can handle both cases based on impl
//           should their be a single Handle type like Arena or a Handle trait impl by multiple types?
//           is multiple types more ergonomic? (shorter type names/less generic params)
//     NOTE: versioning needs to store metadata in arena buffer itself
//     NOTE: versioning and exclusive handle type need to impl different traits (exclusive can't be clone, versioning is copy)
//     => use Wrapper trait to store buffer as Vec<Reclaim::Wrapper<Handle::Wrapper<T>>> with failable get(_mut) methods (e.g. fail on unwrapping free list entry, version mismatch)
// - create Port by consuming Arena (prevents using Arena while Port is around)
// - reclaim Arena from Port only when no other references are around
// - internal staging in arena
//   - Store
//     - handle (de-)allocation and indixing by integer
//     - memory reuse strategies go here
//     - use single backing data type (maybe wrapped if needed by reuse strategy)
//   - InternalArena
//     - handle insert/remove and get(_mut) using Handles
//     - only place where Handles should be created/viewed
//     - responsible for Handle validation (e.g. versioning)
//     - determines the data type stored in Store (e.g. can add extra version data)
//     - has to ensure type correctness/alignment of requested Handle type (and transmute in mixed type arena)
//   - Arena
//     - shallow wrapper around InternalArena
//     - add all utility functionality here
//   TODO: should Ports hold a lock over Arena or InternalArena?
//     (InternalArena common interface, Arena = exclusive access, Port = concurrent access?)
//     - treat as always having alloc lock on Store
//   - Port
//     - alternative to Arena but concurrent
//     - can be locking to individual Port (concurrent write)

mod store;
use store::*;
