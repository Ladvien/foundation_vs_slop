//! **Smart objects** — who can do what, where, and with whom.
//!
//! A [`crate::map::Location`] is an invisible thing that owns a group of props and governs their use.
//! This is the half that runs it: find the seats a location offers, decide which actors fill which
//! roles, and refuse rather than half-start.
//!
//! # Why allocation is the hard half
//!
//! Both chapters this follows say the query is easy and the allocation is not. FFXV (Game AI Pro 3
//! ch.35) is precise about why: *"Since each actor can satisfy multiple roles and each role may
//! require multiple actors, the problem is NP-hard (Gerkey and Mataric 2004)."*
//!
//! And equally precise about the way out, which is to stop trying to solve it:
//!
//! > *"the specific problem instances encountered in interaction scripts are typically very small and
//! > simple, meaning rarely more than three or four distinct roles and rarely more than five actors.
//! > Furthermore, we do not require the resulting allocation to be optimal with respect to some
//! > fitness function. Indeed, we can even allow the role allocation to fail occasionally. Thus we can
//! > formulate role allocation as a Monte-Carlo algorithm by randomizing its input. After
//! > randomization, role allocation simply assigns NPCs greedily to roles until the lower bound of the
//! > respective cardinality is reached. If a role-cardinality cannot be satisfied in this way, the
//! > allocation fails immediately. Subsequently, potentially remaining NPCs are assigned until the
//! > maximum cardinality is reached or no more NPCs can be added."*
//!
//! That is [`allocate`], line for line. The one adaptation is the word *randomizing*.
//!
//! # Randomized, and reproducible
//!
//! FFXV shuffles with whatever RNG is to hand. This project cannot: `tests/determinism_lint.rs`
//! exists because a "stable enough" ordering is how the same seed produced two different runs. So the
//! shuffle is a [`DetRng`] draw over a list that has already been put in a **total** order — actors by
//! id, seats by `(prop, socket)` — and the randomization becomes a property of the seed rather than of
//! whatever order a query happened to return.
//!
//! The distinction matters and is easy to lose: randomizing an *arbitrarily ordered* list is still
//! arbitrary. Sorting first is what makes the Monte-Carlo step a decision the seed owns.
//!
//! # Main gates the scene; Supporting does not
//!
//! FFXV fails on any unmet cardinality. Smart Zones (Game AI Pro 2 ch.11) stratifies instead —
//! *"Main roles are essential… The scene won't start unless all the main roles are fulfilled"*, while
//! supporting roles are favourable and extras optional — and this schema carries both concepts, so it
//! uses both: an unmet [`RoleKind::Main`] minimum aborts immediately, and an unmet Supporting or Extra
//! minimum fills what it can. A `Main` role with `min: 0` is refused at map validation, because that
//! is a Supporting role that has been mislabelled.
//!
//! # Exclusivity is the caller's, and it is not optional
//!
//! *"each NPC only participates in at most one script at a time, and smart locations have exclusive
//! ownership over their props"* — which is what lets scenes run concurrently without any locking at
//! all. [`allocate`] takes only the actors that are free and only the seats that are free; keeping
//! that true is [`Booking`]'s job.

use crate::descriptor::Descriptor;
use crate::library::Library;
use crate::map::{Interaction, Location, Map, RoleKind, RoleSlot};
use crate::rng::DetRng;
use crate::vocab::{Can, Vocabularies};

/// Every role requirement in a map, resolved to masks once at load.
///
/// Once, because a capability token is a property of the map and not of this frame — resolving it per
/// allocation would be a string compare in the middle of the one loop that runs every time anybody
/// looks for something to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoleMasks {
    /// `(location id, verb)` to one mask per role, in `Interaction::roles` order.
    by: Vec<(String, String, Vec<u64>)>,
}

impl RoleMasks {
    /// The masks for one interaction, in role order.
    pub fn get(&self, location: &str, verb: &str) -> Option<&[u64]> {
        self.by
            .iter()
            .find(|(l, v, _)| l == location && v == verb)
            .map(|(_, _, m)| m.as_slice())
    }

    pub fn len(&self) -> usize {
        self.by.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by.is_empty()
    }
}

/// Resolve every role requirement in a map, refusing a token the vocabulary does not hold.
///
/// At load, with the rest of the validation, for the reason the two-sided surface check happens there:
/// a misspelled capability makes a scene that silently never starts, and *"the galley scene never
/// runs"* is the least debuggable sentence a content bug can produce.
pub fn resolve_roles(map: &Map, vocab: &Vocabularies) -> Result<RoleMasks, String> {
    let mut by = Vec::new();
    for loc in &map.locations {
        for interaction in &loc.interactions {
            let site = format!("{}/{}", loc.id, interaction.verb);
            let mut masks = Vec::with_capacity(interaction.roles.len());
            for role in &interaction.roles {
                masks.push(vocab.role_mask(role, &site)?);
            }
            by.push((loc.id.clone(), interaction.verb.clone(), masks));
        }
    }
    Ok(RoleMasks { by })
}

/// Someone who could take part.
///
/// `id` is a stable handle the caller owns — an ECS entity's index, a squad member's number. It is
/// the sort key that makes the shuffle reproducible, so it must be **unique** and must not change
/// between frames for the same actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Actor {
    pub id: u64,
    pub can: Can,
}

/// A place to stand, in world space.
#[derive(Clone, Debug, PartialEq)]
pub struct Seat {
    /// The placement offering it.
    pub prop: String,
    /// The socket's id on that prop's descriptor.
    pub socket: String,
    /// Which role may occupy it, if the socket names one.
    pub role: Option<String>,
    /// Where the occupant stands, world metres.
    pub at: (f32, f32, f32),
    /// Which way they face, world degrees.
    pub yaw: f32,
}

/// One actor, in one role, at one seat.
#[derive(Clone, Debug, PartialEq)]
pub struct Filled {
    pub role: String,
    pub actor: u64,
    /// `None` when the role names no `socket_role` — a bystander who needs no marked spot.
    pub seat: Option<Seat>,
}

/// A started interaction: who is doing what, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Cast {
    pub location: String,
    pub verb: String,
    pub filled: Vec<Filled>,
}

impl Cast {
    /// Everyone taking part. Used to mark them busy, and to prove nobody is in two casts.
    pub fn actors(&self) -> impl Iterator<Item = u64> + '_ {
        self.filled.iter().map(|f| f.actor)
    }
}

/// Why a scene did not start.
///
/// A value rather than a log line: *"we can even allow the role allocation to fail occasionally"*, so
/// failure is an ordinary outcome the caller acts on — try again next tick, widen the search — rather
/// than an error to surface. The detail is here because "the galley scene never runs" is otherwise
/// unanswerable.
#[derive(Clone, Debug, PartialEq)]
pub enum Unfilled {
    /// A `Main` role's minimum could not be met.
    Role {
        role: String,
        need: u8,
        found: usize,
    },
    /// The role wants a marked spot and the location's props offer too few.
    Seats {
        role: String,
        need: u8,
        found: usize,
    },
}

impl std::fmt::Display for Unfilled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unfilled::Role { role, need, found } => write!(
                f,
                "role `{role}` needs {need} and {found} of the actors offered can fill it"
            ),
            Unfilled::Seats { role, need, found } => write!(
                f,
                "role `{role}` needs {need} seat(s) and the location's props offer {found}"
            ),
        }
    }
}

/// Every seat a location's props offer, in a total order.
///
/// A socket is authored in the prop's own space, so this is where it becomes a place in the world:
/// rotated by the prop's yaw, offset to its position, lifted to whatever the prop is standing on.
/// `y` is [`crate::stack::resolve_y`]'s answer for the whole map, so a chair on a dais seats people on
/// the dais.
///
/// Ordered by `(prop, socket)` — both unique — because the shuffle below has to randomize a list whose
/// order is decided by the data rather than by iteration.
pub fn seats_of(
    map: &Map,
    library: &Library,
    y: &[f32],
    location: &Location,
) -> Result<Vec<Seat>, String> {
    let mut out = Vec::new();
    for prop_id in &location.props {
        let Some(i) = map.placements.iter().position(|p| &p.id == prop_id) else {
            return Err(format!(
                "location `{}` governs `{prop_id}`, which this map does not place",
                location.id
            ));
        };
        let p = &map.placements[i];
        let d: &Descriptor = library.get(&p.descriptor).ok_or_else(|| {
            format!(
                "location `{}`: `{prop_id}` names descriptor `{}`, which this library does not define",
                location.id, p.descriptor
            )
        })?;
        let Some(&py) = y.get(i) else { continue };

        for s in &d.offers.sockets {
            // The socket's own offset, turned by the prop's yaw. The prop's `front` correction is
            // deliberately NOT applied: a socket is authored against the mesh, so it is already in the
            // frame `front` corrects for, and applying it twice would seat everyone sideways.
            let (sin, cos) = p.yaw.to_radians().sin_cos();
            let (lx, ly, lz) = s.at;
            out.push(Seat {
                prop: prop_id.clone(),
                socket: s.id.clone(),
                role: s.role.clone(),
                at: (
                    map.origin.0 + p.at.0 + lx * cos + lz * sin,
                    py + ly,
                    map.origin.2 + p.at.1 - lx * sin + lz * cos,
                ),
                yaw: p.yaw + s.yaw,
            });
        }
    }
    // A total order, so the shuffle randomizes the data rather than the iteration.
    out.sort_by(|a, b| (&a.prop, &a.socket).cmp(&(&b.prop, &b.socket)));
    Ok(out)
}

/// Fill an interaction's roles from the actors offered.
///
/// See the module docs for the algorithm and the chapter it comes from. In short: shuffle a totally
/// ordered list, greedily reach every role's minimum, abort the moment a `Main` minimum cannot be met,
/// then hand out whoever is left up to each maximum.
///
/// `requires` is the resolved capability mask per role, in `interaction.roles` order — resolved once
/// at load rather than per call, since it is a property of the map and not of this frame.
pub fn allocate(
    location: &Location,
    interaction: &Interaction,
    requires: &[u64],
    actors: &[Actor],
    seats: &[Seat],
    rng: &mut impl DetRng,
) -> Result<Cast, Unfilled> {
    // **Randomize the input.** Sorted first: randomizing an arbitrarily ordered list is still
    // arbitrary, and the sort is what makes this a decision the seed owns.
    let mut pool: Vec<Actor> = actors.to_vec();
    pool.sort_unstable();
    shuffle(&mut pool, rng);

    let mut free_seats: Vec<&Seat> = seats.iter().collect();
    let mut filled: Vec<Filled> = Vec::new();
    let mut taken = vec![false; pool.len()];

    // Pass one: every role's lower bound. A `Main` that cannot be met ends it here — before anything
    // has been committed, which is the whole reason the bound is checked before the surplus is spread.
    for (r, role) in interaction.roles.iter().enumerate() {
        let needs = requires.get(r).copied().unwrap_or(0);
        let want = usize::from(role.min);
        let got = take_into(
            &mut filled,
            &pool,
            &mut taken,
            &mut free_seats,
            role,
            needs,
            want,
        );
        if got < want {
            // Supporting is *favourable*, Extra is ambient — neither gates the scene. Only Main does.
            if role.kind != RoleKind::Main {
                continue;
            }
            return Err(match seat_shortage(role, seats, want) {
                Some(found) => Unfilled::Seats {
                    role: role.name.clone(),
                    need: role.min,
                    found,
                },
                None => Unfilled::Role {
                    role: role.name.clone(),
                    need: role.min,
                    found: got,
                },
            });
        }
    }

    // Pass two: whoever is left, up to each maximum. Role order, so a location's own file decides who
    // gets the surplus rather than the shuffle deciding twice.
    for (r, role) in interaction.roles.iter().enumerate() {
        let needs = requires.get(r).copied().unwrap_or(0);
        let already = filled.iter().filter(|f| f.role == role.name).count();
        let room = usize::from(role.max).saturating_sub(already);
        if room > 0 {
            take_into(
                &mut filled,
                &pool,
                &mut taken,
                &mut free_seats,
                role,
                needs,
                room,
            );
        }
    }

    Ok(Cast {
        location: location.id.clone(),
        verb: interaction.verb.clone(),
        filled,
    })
}

/// Assign up to `want` actors to `role`, returning how many were placed.
///
/// Takes from the front of the shuffled pool, which is what makes the greedy pass greedy. An actor is
/// consumed exactly once — `taken` is the reason nobody ends up in two roles of one scene, the same
/// way [`Booking`] is the reason nobody ends up in two scenes.
fn take_into<'a>(
    filled: &mut Vec<Filled>,
    pool: &[Actor],
    taken: &mut [bool],
    free_seats: &mut Vec<&'a Seat>,
    role: &RoleSlot,
    needs: u64,
    want: usize,
) -> usize {
    let mut placed = 0usize;
    for (i, actor) in pool.iter().enumerate() {
        if placed == want {
            break;
        }
        if taken[i] || !actor.can.meets(needs) {
            continue;
        }
        // A role that names a socket role needs a spot; one that does not is a bystander.
        let seat = match &role.socket_role {
            Some(want_role) => {
                let Some(at) = free_seats
                    .iter()
                    .position(|s| s.role.as_deref() == Some(want_role.as_str()))
                else {
                    // No spot left. Not this actor's fault, and no later actor will do better, so
                    // stop rather than walk the rest of the pool.
                    break;
                };
                Some(free_seats.remove(at).clone())
            }
            None => None,
        };
        taken[i] = true;
        filled.push(Filled {
            role: role.name.clone(),
            actor: actor.id,
            seat,
        });
        placed += 1;
    }
    placed
}

/// If this role wants marked spots and the location does not offer enough, how many it offers.
///
/// Separating "nobody who can do it" from "nowhere to put them" is the difference between an author
/// adding a chair and an author widening a search radius.
fn seat_shortage(role: &RoleSlot, seats: &[Seat], want: usize) -> Option<usize> {
    let want_role = role.socket_role.as_deref()?;
    let have = seats
        .iter()
        .filter(|s| s.role.as_deref() == Some(want_role))
        .count();
    (have < want).then_some(have)
}

/// Fisher–Yates over a [`DetRng`].
///
/// Written out rather than pulled from `rand`, for the reason this crate hand-rolls its GLB reader:
/// the shuffle is on the determinism path, and a dependency's shuffle is one upgrade away from being
/// a different permutation for the same seed.
fn shuffle<T>(items: &mut [T], rng: &mut impl DetRng) {
    if items.len() < 2 {
        return;
    }
    for i in (1..items.len()).rev() {
        items.swap(i, rng.below(i + 1));
    }
}

/// Who is busy, and which seats are spoken for.
///
/// *"each NPC only participates in at most one script at a time, and smart locations have exclusive
/// ownership over their props"* — and that sentence is not a nicety, it is what lets scenes run
/// concurrently *"without the need for thread synchronization."* Booking is where it is kept true.
#[derive(Clone, Debug, Default)]
pub struct Booking {
    /// Locations currently running something, and the cast running it.
    running: Vec<Cast>,
}

impl Booking {
    pub fn new() -> Booking {
        Booking::default()
    }

    /// Is this location already running an interaction?
    ///
    /// *"Although a smart location is running a script, it will not start a second one to avoid
    /// concurrent access to its resources."*
    pub fn is_busy(&self, location: &str) -> bool {
        self.running.iter().any(|c| c.location == location)
    }

    /// Is this actor already in a scene?
    pub fn is_engaged(&self, actor: u64) -> bool {
        self.running.iter().any(|c| c.actors().any(|a| a == actor))
    }

    /// The actors from `offered` who are free to take part.
    pub fn free<'a>(&self, offered: &'a [Actor]) -> Vec<Actor> {
        offered
            .iter()
            .copied()
            .filter(|a| !self.is_engaged(a.id))
            .collect()
    }

    /// Start a cast, or say why it cannot be started.
    ///
    /// Refuses rather than overwriting. A caller that double-books is a caller with a bug, and finding
    /// it here beats finding it as one agent playing two animations.
    pub fn start(&mut self, cast: Cast) -> Result<(), String> {
        if self.is_busy(&cast.location) {
            return Err(format!(
                "location `{}` is already running an interaction — a location owns its props, so a \
                 second scene would be two casts using one table",
                cast.location
            ));
        }
        if let Some(a) = cast.actors().find(|a| self.is_engaged(*a)) {
            return Err(format!(
                "actor {a} is already in a scene — an actor takes part in at most one at a time"
            ));
        }
        self.running.push(cast);
        Ok(())
    }

    /// End whatever `location` was running, returning it.
    pub fn finish(&mut self, location: &str) -> Option<Cast> {
        let at = self.running.iter().position(|c| c.location == location)?;
        Some(self.running.remove(at))
    }

    /// Everything currently running.
    pub fn casts(&self) -> &[Cast] {
        &self.running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Extent, Offers, Socket};
    use crate::library::LIBRARY_VERSION;
    use crate::map::{Effect, Interaction, Placed, RoleKind, RoleSlot};
    use crate::rng::seeded;

    const EAT: u64 = 1;
    const COOK: u64 = 2;

    fn table_with_seats(n: usize) -> Descriptor {
        Descriptor {
            id: "table".into(),
            mesh: Some("table.glb".into()),
            extent: Extent {
                footprint: Some((1.6, 0.8)),
                height: Some(0.8),
            },
            offers: Offers {
                surfaces: vec!["worktop".into()],
                faces: Vec::new(),
                sockets: (0..n)
                    .map(|i| Socket {
                        id: format!("seat_{i}"),
                        role: Some("diner".into()),
                        at: (0.0, 0.45, -0.5 + i as f32),
                        yaw: 0.0,
                    })
                    .collect(),
            },
            ..Descriptor::default()
        }
    }

    fn library(n_seats: usize) -> Library {
        Library {
            version: LIBRARY_VERSION,
            note: None,
            descriptors: vec![table_with_seats(n_seats)],
        }
    }

    fn map_with_table() -> Map {
        Map {
            name: "galley".into(),
            placements: vec![Placed {
                id: "t1".into(),
                descriptor: "table".into(),
                at: (2.0, 3.0),
                ..Placed::default()
            }],
            ..Map::default()
        }
    }

    fn role(name: &str, kind: RoleKind, min: u8, max: u8, socket: Option<&str>) -> RoleSlot {
        RoleSlot {
            name: name.to_owned(),
            kind,
            min,
            max,
            socket_role: socket.map(str::to_owned),
            requires: Vec::new(),
        }
    }

    fn dinner(roles: Vec<RoleSlot>) -> Interaction {
        Interaction {
            verb: "eat".into(),
            roles,
            guard: None,
            effects: vec![Effect::Restore {
                drive: "stamina".into(),
                rate: 0.2,
            }],
            note: None,
        }
    }

    fn location(props: &[&str], interactions: Vec<Interaction>) -> Location {
        Location {
            id: "galley_table_1".into(),
            props: props.iter().map(|p| (*p).to_owned()).collect(),
            interactions,
            note: None,
        }
    }

    fn actors(spec: &[(u64, u64)]) -> Vec<Actor> {
        spec.iter()
            .map(|(id, can)| Actor {
                id: *id,
                can: Can(*can),
            })
            .collect()
    }

    fn seats(n: usize) -> Vec<Seat> {
        let map = map_with_table();
        let lib = library(n);
        let y = crate::stack::resolve_y(&map, &lib).unwrap_or_else(|e| panic!("{e}"));
        seats_of(&map, &lib, &y, &location(&["t1"], vec![])).unwrap_or_else(|e| panic!("{e}"))
    }

    /// **The plan's Stage 6b gate: four agents fill a four-seat table.** No double-booking — every
    /// actor once, every seat once.
    #[test]
    fn four_agents_fill_a_four_seat_table_without_double_booking() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 1, 4, Some("diner"))]);
        let cast = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[0],
            &actors(&[(1, EAT), (2, EAT), (3, EAT), (4, EAT)]),
            &seats(4),
            &mut seeded(7),
        )
        .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(cast.filled.len(), 4);

        let mut who: Vec<u64> = cast.actors().collect();
        who.sort_unstable();
        assert_eq!(who, vec![1, 2, 3, 4], "every actor exactly once");

        let mut where_: Vec<&str> = cast
            .filled
            .iter()
            .filter_map(|f| f.seat.as_ref().map(|s| s.socket.as_str()))
            .collect();
        where_.sort_unstable();
        where_.dedup();
        assert_eq!(where_.len(), 4, "every seat exactly once");
    }

    /// A fifth diner has nowhere to sit, and the table seats four rather than stacking two on a chair.
    #[test]
    fn a_table_seats_as_many_as_it_has_chairs() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 1, 8, Some("diner"))]);
        let cast = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[0],
            &actors(&[(1, EAT), (2, EAT), (3, EAT), (4, EAT), (5, EAT)]),
            &seats(4),
            &mut seeded(7),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cast.filled.len(), 4);
    }

    /// **A Main role gates the scene.** Nobody who can eat means no dinner — and the failure says
    /// which role and how short it was, because "the galley scene never runs" is otherwise
    /// unanswerable.
    #[test]
    fn an_unfillable_main_role_stops_the_scene() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 2, 4, Some("diner"))]);
        let err = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[EAT],
            &actors(&[(1, COOK)]),
            &seats(4),
            &mut seeded(7),
        )
        .err()
        .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(
            err,
            Unfilled::Role {
                role: "diner".into(),
                need: 2,
                found: 0
            }
        );
    }

    /// Too few chairs is a different problem from too few people, and says so: one is answered by
    /// authoring a chair, the other by looking further for actors.
    #[test]
    fn too_few_seats_is_a_different_failure_from_too_few_actors() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 3, 4, Some("diner"))]);
        let err = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[0],
            &actors(&[(1, EAT), (2, EAT), (3, EAT)]),
            &seats(2),
            &mut seeded(7),
        )
        .err()
        .unwrap_or_else(|| panic!("expected a refusal"));
        assert_eq!(
            err,
            Unfilled::Seats {
                role: "diner".into(),
                need: 3,
                found: 2
            }
        );
    }

    /// **Supporting is favourable, not required** — Smart Zones' strata. Dinner happens without a
    /// server; it does not happen without a diner.
    #[test]
    fn a_supporting_role_that_cannot_be_filled_does_not_stop_the_scene() {
        let interaction = dinner(vec![
            role("diner", RoleKind::Main, 1, 4, Some("diner")),
            role("server", RoleKind::Supporting, 1, 1, None),
        ]);
        let mut server = role("server", RoleKind::Supporting, 1, 1, None);
        server.requires = vec!["cook".into()];
        let interaction = Interaction {
            roles: vec![interaction.roles[0].clone(), server],
            ..interaction
        };

        let cast = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[EAT, COOK],
            // Two diners, no cook among them.
            &actors(&[(1, EAT), (2, EAT)]),
            &seats(4),
            &mut seeded(7),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(cast.filled.iter().all(|f| f.role == "diner"));
        assert_eq!(cast.filled.len(), 2);
    }

    /// **All of the requirements, not any.** A role wanting somebody who can cook *and* eat must not
    /// accept somebody who can only eat.
    #[test]
    fn a_role_requiring_two_capabilities_wants_both() {
        let mut chef = role("chef", RoleKind::Main, 1, 1, None);
        chef.requires = vec!["eat".into(), "cook".into()];
        let interaction = dinner(vec![chef]);

        let err = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[EAT | COOK],
            &actors(&[(1, EAT), (2, COOK)]),
            &seats(0),
            &mut seeded(7),
        )
        .err();
        assert!(err.is_some(), "neither actor can do both");

        let cast = allocate(
            &location(&["t1"], vec![]),
            &interaction,
            &[EAT | COOK],
            &actors(&[(1, EAT), (2, EAT | COOK)]),
            &seats(0),
            &mut seeded(7),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cast.filled[0].actor, 2);
    }

    /// **Same seed, same cast.** The Monte-Carlo step is a decision the seed owns, which is the one
    /// adaptation this makes to the chapter's algorithm.
    #[test]
    fn the_same_seed_allocates_the_same_way() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 1, 2, Some("diner"))]);
        let who = |seed: u64, order: &[(u64, u64)]| -> Vec<u64> {
            allocate(
                &location(&["t1"], vec![]),
                &interaction,
                &[0],
                &actors(order),
                &seats(4),
                &mut seeded(seed),
            )
            .map(|c| c.actors().collect())
            .unwrap_or_default()
        };

        let forwards = [(1, EAT), (2, EAT), (3, EAT), (4, EAT)];
        let backwards = [(4, EAT), (3, EAT), (2, EAT), (1, EAT)];
        assert_eq!(who(11, &forwards), who(11, &forwards));
        // **And the order the caller happened to hand them over does not matter.** This is the whole
        // reason the pool is sorted before it is shuffled: randomizing an arbitrary order is still
        // arbitrary, and a query returning actors in a different order would otherwise seat a
        // different pair.
        assert_eq!(who(11, &forwards), who(11, &backwards));
        // A different seed is allowed to differ — that is what makes it Monte-Carlo — and over these
        // seeds it does, which is what proves the shuffle is doing anything at all.
        let seeds: Vec<Vec<u64>> = (0..24).map(|s| who(s, &forwards)).collect();
        assert!(
            seeds.iter().any(|s| s != &seeds[0]),
            "every seed produced the same cast — the shuffle is not shuffling"
        );
    }

    /// A socket is authored in the prop's own space, so a turned table seats people around where it
    /// actually is.
    #[test]
    fn a_seat_is_placed_by_its_props_position_and_yaw() {
        let mut map = map_with_table();
        let lib = library(1);
        let y = crate::stack::resolve_y(&map, &lib).unwrap_or_else(|e| panic!("{e}"));
        let s = &seats_of(&map, &lib, &y, &location(&["t1"], vec![]))
            .unwrap_or_else(|e| panic!("{e}"))[0];
        // Socket 0 is at local (0, 0.45, -0.5); the table stands at map (2, 3) on the floor.
        assert!((s.at.0 - 2.0).abs() < 1e-5, "{:?}", s.at);
        assert!((s.at.1 - 0.45).abs() < 1e-5, "{:?}", s.at);
        assert!((s.at.2 - 2.5).abs() < 1e-5, "{:?}", s.at);

        // Turn the table a quarter turn and the seat swings with it.
        map.placements[0].yaw = 90.0;
        let s = &seats_of(&map, &lib, &y, &location(&["t1"], vec![]))
            .unwrap_or_else(|e| panic!("{e}"))[0];
        assert!((s.at.0 - 1.5).abs() < 1e-5, "{:?}", s.at);
        assert!((s.at.2 - 3.0).abs() < 1e-5, "{:?}", s.at);
        assert!((s.yaw - 90.0).abs() < 1e-5);
    }

    /// A location whose prop the map does not place is a hole, and says which one.
    #[test]
    fn a_location_governing_a_missing_prop_is_refused() {
        let map = map_with_table();
        let lib = library(2);
        let y = crate::stack::resolve_y(&map, &lib).unwrap_or_else(|e| panic!("{e}"));
        let err = seats_of(&map, &lib, &y, &location(&["t1", "t9"], vec![]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("t9"), "{err}");
    }

    /// **Exclusivity.** One scene per location, one scene per actor — the property that lets scenes
    /// run concurrently with no locking at all.
    #[test]
    fn a_location_runs_one_scene_and_an_actor_joins_one() {
        let cast = |loc: &str, who: &[u64]| Cast {
            location: loc.to_owned(),
            verb: "eat".into(),
            filled: who
                .iter()
                .map(|a| Filled {
                    role: "diner".into(),
                    actor: *a,
                    seat: None,
                })
                .collect(),
        };

        let mut booking = Booking::new();
        booking
            .start(cast("galley_table_1", &[1, 2]))
            .unwrap_or_else(|e| panic!("{e}"));

        // The same table cannot start a second scene: it owns its props.
        let err = booking
            .start(cast("galley_table_1", &[3]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("already running"), "{err}");

        // Nor can a busy actor join one elsewhere.
        let err = booking
            .start(cast("bunk_2", &[2]))
            .err()
            .unwrap_or_default();
        assert!(err.contains("already in a scene"), "{err}");

        // Somebody free, somewhere free, is fine.
        booking
            .start(cast("bunk_2", &[3]))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(booking.casts().len(), 2);

        // `free` is what keeps `allocate`'s input honest.
        let offered = actors(&[(1, EAT), (2, EAT), (3, EAT), (4, EAT)]);
        assert_eq!(
            booking.free(&offered).iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![4]
        );

        // And it all comes back when the scene ends.
        assert!(booking.finish("galley_table_1").is_some());
        assert!(!booking.is_engaged(1));
        assert!(!booking.is_busy("galley_table_1"));
        assert!(booking.finish("galley_table_1").is_none());
    }

    /// **No deadlock**: whatever the outcome, every actor is either in exactly one cast or free, and
    /// nobody is stranded holding a seat nobody released. Run over many seeds and many pool sizes,
    /// because the failure this guards is an ordering one and a single seed proves nothing.
    #[test]
    fn repeated_allocation_never_strands_an_actor_or_a_seat() {
        let interaction = dinner(vec![role("diner", RoleKind::Main, 1, 4, Some("diner"))]);
        for seed in 0..64u64 {
            for n in 0..6usize {
                let mut booking = Booking::new();
                let offered: Vec<Actor> = (1..=6)
                    .map(|id| Actor {
                        id,
                        can: Can(EAT),
                    })
                    .collect();

                let free = booking.free(&offered);
                if let Ok(cast) = allocate(
                    &location(&["t1"], vec![]),
                    &interaction,
                    &[0],
                    &free,
                    &seats(n),
                    &mut seeded(seed * 31 + n as u64),
                ) {
                    let seated = cast.filled.len();
                    booking.start(cast).unwrap_or_else(|e| panic!("{e}"));
                    // Never more people than chairs, and never a chair filled twice.
                    assert!(seated <= n, "seated {seated} at {n} chair(s)");
                    assert_eq!(
                        booking.casts()[0].actors().count(),
                        seated,
                        "an actor appears twice in one cast"
                    );
                }
                // Whatever happened, every actor is accounted for exactly once.
                let engaged = offered.iter().filter(|a| booking.is_engaged(a.id)).count();
                assert_eq!(engaged + booking.free(&offered).len(), offered.len());
            }
        }
    }
}
