## ISSUES
- Furniture spawns outside of wall boundaries.
- Furniture spawns halfway through walls
- Toliet and sink need rules to be together, and close to the walls
- TV, lamps, small potted plants all need to stack on tables and desks
- The wall cutaways don't change when the player rotates the map


### TODO
- Ensure the crabs have flocking like behaviors and don't pile up
- Make blood pools relate to the size of the mesh and/or weight of the mesh
- **Stealth pounce:** gate the leap on the target's facing — stalk to the blind side and only pounce when prey isn't looking. (Now: range + cooldown only.)
- **Dynamic castes:** let crabs re-role between scout and assault as swarm needs shift. (Now: fixed at birth.)

## Crabs

Wall-climbing swarm — ~40 to start, breed up to ~5000, from 4 nests in far rooms. One crab dies to a shot or two; the threat is the mass. They climb walls like floors, so geometry won't corner them.

- **Forage & haul.** Corpse gibs emit a *meat* scent; hungry crabs climb the gradient to a pile. Heavy chunks need a crew to lift and haul home (along walls). Delivered meat is the nest's only breeding fuel — well-fed nests birth ~10× faster, starved ones stop. Destroy the gibs to cut off reinforcements.
- **Numbers kill.** Under ~5 crabs on a target, zero damage. Past that the bite scales super-linearly — a pile shreds a unit in seconds. They cling to the **back**, out of the gun's reach, spread over the whole body.
- **Pounce.** Near a unit, hunker then leap a ballistic arc (~10 body lengths), biting on landing. Same critical-mass rule: a lone leaper lands but does no damage.
- **Scent-only coordination (stigmergy).** *Meat* draws foragers; *blood* from kills pulls the swarm (and the Smiley); *threat* from gunfire frightens them; *crowding* caps nest breeding; scouts lay a directional *rally* pheromone at spotted prey.
- **Scouts recruit.** ~1 in 5 is a scout: roams fast, and on spotting prey shadows it and lays rally pheromone at its live position, pulling the swarm in. Lose sight → pheromone evaporates → attack calls off. Scouts don't fight.
- **Fear scatters them.** Gunfire raises *threat* → fear → flee (dropping loads); it decays and they resume. Overrides: a nest under attack goes **berserk**, and crabs in a fresh rally beacon push through fire.


## Smiley
Is cow like. It should have a saddish look on its face, until it sees a squad member.  To which it moves towards the closest squad member, with its eyes and smiling getting bigger the closer it is.  If the Smiley is in LoS of any squad member and its attacked, it looks scared and runs away.  But if no squad member is looking directly at it (raytracing area), _and_ its attacked, it looks angry, then switches to a different shader, still in a sphere shaped, but shoots a lighting bolt at the enemy, instantly killing it, then switches back to its angry face relaxing, if that was the last enemy.  The idea is to give an idea of how comisically powerful this entity is, but it is trying to conceal this from you.  Like, it's so lonely, it wants to try to keep you around, even though it could kill you instatly.  Like it knows how cognitohazardous it is to you and trying to save you from that.


## Favorite Shaders
- https://www.shadertoy.com/view/lsXcWn
- https://www.shadertoy.com/view/4lfXRf
- https://www.shadertoy.com/view/lsKyWV
- https://www.shadertoy.com/view/XljGDz
- https://www.shadertoy.com/view/4slXW7
- https://www.shadertoy.com/view/WXyczK
- https://www.shadertoy.com/view/MdG3Dd
- https://www.shadertoy.com/view/4ldGDB
- https://www.shadertoy.com/view/MsVcRy
- https://www.shadertoy.com/view/ld3SDl
- https://www.shadertoy.com/view/4tSXWt
- https://www.shadertoy.com/view/XfXGz4
- https://www.shadertoy.com/view/XsXGRS
- https://www.shadertoy.com/view/fljBWc
- https://www.shadertoy.com/view/WtSBzh
- https://www.shadertoy.com/view/l3cfW4
- https://www.shadertoy.com/view/MsVXWW
- https://www.shadertoy.com/view/mtScRc
- https://www.shadertoy.com/view/MdfGRr
- https://www.shadertoy.com/view/WsV3D1
- https://www.shadertoy.com/view/MslGD8
- https://www.shadertoy.com/view/XtyXzW
- https://www.shadertoy.com/view/ld2GRz
- https://www.shadertoy.com/view/Mld3Rn
- https://www.shadertoy.com/view/MsGSRd
- https://www.shadertoy.com/view/4dl3zn
- https://www.shadertoy.com/view/lllBDM
- https://www.shadertoy.com/view/lssGRM
- https://www.shadertoy.com/view/MsfGRr
- https://www.shadertoy.com/view/WlVyRV
- https://www.shadertoy.com/view/4sXBRn
- https://www.shadertoy.com/view/XllGW4
- https://www.shadertoy.com/view/ldd3DB (boid)
- https://www.shadertoy.com/view/MstXWS
- https://www.shadertoy.com/view/XsjXRm
- https://www.shadertoy.com/view/Mss3WN
- https://www.shadertoy.com/view/3tXXRn
- https://www.shadertoy.com/view/4tySDW
- https://www.shadertoy.com/view/4t2SWW
- https://www.shadertoy.com/view/flfyRS
- https://www.shadertoy.com/view/XsG3z1

### TV Distortion
- https://www.shadertoy.com/view/XtBXDt
- https://www.shadertoy.com/view/XtK3W3
- https://www.shadertoy.com/view/Ms3XWH
- https://www.shadertoy.com/view/cdG3Wd
- https://www.shadertoy.com/view/4dsGD7
- https://www.shadertoy.com/view/ldjGzV
- https://www.shadertoy.com/view/sltBWM
- https://www.shadertoy.com/view/XsfGDl
- https://www.shadertoy.com/view/ltf3WB

## Glitch
- https://www.shadertoy.com/view/MltBzf

## Blood
- https://www.shadertoy.com/view/4ttXzj (blood cells)

## Scanner
- https://www.shadertoy.com/view/fdBfD1

## Liquid in Glass
- https://www.shadertoy.com/view/3tfcRS

## Water Element
- https://www.shadertoy.com/view/NdS3zK

## Glass
- https://www.shadertoy.com/view/4s2Gz3

## Copper Flesh
- https://www.shadertoy.com/view/WljSWz

## Drippy
- https://www.shadertoy.com/view/MstGWX

## Monster Skin
- https://www.shadertoy.com/view/7tjSWy (eyeballs)


## "Grass"
- https://www.shadertoy.com/view/XtyGzh

## Mandelbrot Portal
- https://www.shadertoy.com/view/4dXGDX


## Lava Shooter
- https://www.shadertoy.com/view/WdtXzs

## Cloudy Explosion
- https://www.shadertoy.com/view/4lfSzs

## Lens Distortion
- https://www.shadertoy.com/view/WfS3Dd

## Coolest Skin Ever
- https://www.shadertoy.com/view/Xs2cRD

## Smoke Rings
- https://www.shadertoy.com/view/4dVXDt


## Rift
- https://www.shadertoy.com/view/dsByWy
- https://www.shadertoy.com/view/lXfXWS
- https://www.shadertoy.com/view/WsGfWw


## Noodle Shape Fill
- https://www.shadertoy.com/view/ssjyWc