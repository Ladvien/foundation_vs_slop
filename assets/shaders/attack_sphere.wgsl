// The Smiley's "true form" — the shader it flips to the instant it is attacked while UNOBSERVED (see
// `enemy::smiley_reflex`). A recursive ray-marched fractal sphere with spinning teeth/fins: still a
// sphere, but wrong — the mask coming off only when no one is watching (audience-effect concealment:
// Hamilton & Cañigueral, "The Role of Eye Gaze During Natural Social Interactions", Front. Psychol.
// 2019, DOI 10.3389/fpsyg.2019.00560).
//
// Ported to WGSL from the Shadertoy "Sphere Gears" / kaleidoscopic-IFS sphere by Otavio Good, which the
// author dedicated to the public domain:
//   License CC0 - http://creativecommons.org/publicdomain/zero/1.0/  (c) Otavio Good.
// Algorithm unchanged; adapted for an in-world camera-facing billboard quad the same way `smiley.wgsl`
// adapted BigWings' face:
//   * iTime  -> globals.time
//   * fragCoord/iResolution -> the quad's [0,1] uv, remapped to [-1,1] (aspect fixed to 1.0 — square quad)
//   * iMouse dropped (the camera auto-orbits off localTime); iChannel env sampling was already procedural
//   * a ray that MISSES the sphere outputs alpha 0 (coverage-as-alpha) so only the orb composites over
//     the scene and the square quad vanishes under AlphaMode::Blend
//   * `charge` (uniform) fades the orb in as it powers up and out as it relaxes
// WGSL notes: GLSL's module-level mutable globals become `var<private>`; `saturate` (no WGSL builtin) is
// `sat`; the SPLIT_ANIM path is compiled out in the original, so it is simply omitted here.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

// No material uniforms: the orb is a hard on/off keyed on the entity's `Visibility` (Rust side), so there
// is nothing to fade — the fragment just outputs coverage-as-alpha.

const RECURSION_LEVELS: i32 = 4;
const inner: f32 = 0.333;
const outness: f32 = 1.414;
// normalize(vec3(-1.0)) precomputed (WGSL const cannot call normalize).
const diagN: vec3<f32> = vec3<f32>(-0.5773503, -0.5773503, -0.5773503);

// Animation state shared across the distance-field functions (GLSL module globals).
var<private> localTime: f32 = 0.0;
var<private> spinTime: f32 = 0.0;
var<private> finWidth: f32 = 0.0;
var<private> teeth: f32 = 0.0;
var<private> globalTeeth: f32 = 0.0;
var<private> cut: f32 = 0.77;
var<private> camPos: vec3<f32> = vec3<f32>(0.0);
var<private> camLookat: vec3<f32> = vec3<f32>(0.0);

fn sat(x: f32) -> f32 {
    return clamp(x, 0.0, 1.0);
}

fn RotateY(v: vec3<f32>, rad: f32) -> vec3<f32> {
    let c = cos(rad);
    let s = sin(rad);
    return vec3<f32>(c * v.x - s * v.z, v.y, s * v.x + c * v.z);
}

// polynomial smooth min (k = 0.1)
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn matMin(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    if (a.x < b.x) {
        return a;
    }
    return b;
}

fn sphereIter(p_in: vec3<f32>, radius_in: f32, subA: f32) -> vec2<f32> {
    var p = p_in;
    var radius = radius_in;
    finWidth = 0.1;
    teeth = globalTeeth;
    var blender = 0.25;
    var acc = vec2<f32>(1000000.0, 0.0);
    for (var i = 0; i < RECURSION_LEVELS; i = i + 1) {
        // main sphere
        var d = length(p) - radius * outness;
        // calc new position at 8 vertices of cube, scaled
        var corners = abs(p) + diagN * radius;
        let lenCorners = length(corners);
        // subtract out main sphere hole, mirrored on all axises
        var subtracter = lenCorners - radius * subA;
        // mirrored fins that go through all vertices of the cube
        let ap = abs(p) * 0.7071; // 1/sqrt(2) to keep distance field normalized
        subtracter = max(subtracter, -(abs(ap.x - ap.y) - finWidth));
        subtracter = max(subtracter, -(abs(ap.y - ap.z) - finWidth));
        subtracter = max(subtracter, -(abs(ap.z - ap.x) - finWidth));
        // subtract sphere from fins, animated so they are like teeth
        subtracter = min(subtracter, lenCorners - radius * subA + teeth);
        // smoothly subtract out that whole complex shape
        d = -smin(-d, subtracter, blender);
        acc = matMin(acc, vec2<f32>(d, f32(i)));
        corners = RotateY(corners, spinTime * 0.25 / blender);
        // Simple rotate 90 degrees on X axis to keep things fresh
        p = vec3<f32>(corners.x, corners.z, -corners.y);
        // Scale things for the next iteration / recursion-like-thing
        radius = radius * inner;
        teeth = teeth * inner;
        finWidth = finWidth * inner;
        blender = blender * inner;
    }
    // Bring in the smallest-sized sphere (`acc`, renamed from `final` — a WGSL reserved keyword)
    let dFinal = length(p) - radius * outness;
    acc = matMin(acc, vec2<f32>(dFinal, 6.0));
    return acc;
}

fn DistanceToObject(p: vec3<f32>) -> vec2<f32> {
    return sphereIter(p, 5.2 / outness, cut);
}

// dir MUST be normalized first
fn SphereIntersect(pos: vec3<f32>, dir: vec3<f32>, spherePos: vec3<f32>, rad: f32) -> f32 {
    let radialVec = pos - spherePos;
    let b = dot(radialVec, dir);
    let c = dot(radialVec, radialVec) - rad * rad;
    let h = b * b - c;
    if (h < 0.0) {
        return -1.0;
    }
    return -b - sqrt(h);
}

// Procedural environment map: a giant overhead softbox plus side lights, faded bottom-to-top.
fn GetEnvColor2(rayDir: vec3<f32>, sunDir: vec3<f32>) -> vec3<f32> {
    var env = vec3<f32>(dot(-rayDir, sunDir) * 0.5 + 0.5);
    env = env * 0.125;
    if ((rayDir.y > abs(rayDir.x) * 1.0) && (rayDir.y > abs(rayDir.z * 0.25))) {
        env = vec3<f32>(2.0) * rayDir.y;
    }
    // Overhead softbox, projected onto the xz plane by dividing through rayDir.y. Only upward rays
    // can catch it; for downward/horizon rays (rayDir.y <= 0) the old `max(0.0, rayDir.y)` denominator
    // was exactly 0, so axis-aligned rays hit 0.0/0.0 = NaN (a stray speckle). Gate the box on upward
    // rays instead: downward rays get a huge roundBox and contribute nothing, matching the intent.
    var roundBox = 1.0e9;
    if (rayDir.y > 1.0e-4) {
        roundBox = length(max(abs(rayDir.xz) / rayDir.y - vec2<f32>(0.9, 4.0), vec2<f32>(0.0))) - 0.1;
    }
    env = env + vec3<f32>(0.8) * pow(sat(1.0 - roundBox * 0.5), 6.0);
    // purple lights from side
    env = env + vec3<f32>(8.0, 6.0, 7.0) * sat(0.001 / (1.0 - abs(rayDir.x)));
    // yellow lights from side
    env = env + vec3<f32>(8.0, 7.0, 6.0) * sat(0.001 / (1.0 - abs(rayDir.z)));
    return env;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    localTime = globals.time;

    // Quad UVs are [0,1] with origin top-left; center to [-1,1] and flip y so the orb is upright.
    var uv = mesh.uv * 2.0 - vec2<f32>(1.0);
    uv.y = -uv.y;
    let zoom = 1.7;
    uv = uv / zoom;

    let camUp = vec3<f32>(0.0, 1.0, 0.0);
    camLookat = vec3<f32>(0.0, 0.0, 0.0);

    // Auto-orbit (iMouse fixed at 0): a slow yaw + gentle bob.
    let mx = -0.7 + localTime * 3.1415 * 0.0625 * 0.666;
    let my = -sin(localTime * 0.31) * 0.5;
    camPos = vec3<f32>(0.0);
    camPos = camPos + vec3<f32>(cos(my) * cos(mx), sin(my), cos(my) * sin(mx)) * 12.2;

    let camVec = normalize(camLookat - camPos);
    let sideNorm = normalize(cross(camUp, camVec));
    let upNorm = cross(camVec, sideNorm);
    let worldFacing = camPos + camVec;
    // Aspect fixed to 1.0 — the face quad is square (FACE_SIZE x FACE_SIZE).
    let worldPix = worldFacing + uv.x * sideNorm + uv.y * upNorm;
    let rayVec = normalize(worldPix - camPos);

    // ----- Animate -----
    localTime = globals.time * 0.5;
    // triangle-ish wave with flat tops/bottoms, period 1
    let rampStep0 = min(3.0, max(1.0, abs((fract(localTime) - 0.5) * 1.0) * 8.0)) * 0.5 - 0.5;
    let rampStep = smoothstep(0.0, 1.0, rampStep0);
    // lopsided triangle wave — up for 3 units, down for 1
    let tri = fract(localTime + 0.125) - 0.25;
    let step31 = (max(0.0, tri) - min(0.0, tri) * 3.0) * 0.333;
    spinTime = step31 + localTime;
    globalTeeth = rampStep * 0.99;
    cut = max(0.48, min(0.77, localTime));

    var distAndMat = vec2<f32>(0.5, 0.0);
    var t = 0.0;
    let maxDepth = 24.0;
    var pos = vec3<f32>(0.0);

    // Bounding-sphere early out so we only ray-march near the object.
    let hit = SphereIntersect(camPos, rayVec, vec3<f32>(0.0), 5.6);
    if (hit >= 0.0) {
        t = hit;
        for (var i = 0; i < 290; i = i + 1) {
            pos = camPos + rayVec * t;
            distAndMat = DistanceToObject(pos);
            t = t + distAndMat.x * 0.7;
            if ((t > maxDepth) || (abs(distAndMat.x) < 0.0025)) {
                break;
            }
        }
    } else {
        t = maxDepth + 1.0;
        distAndMat.x = 1000000.0;
    }

    let sunDir = normalize(vec3<f32>(3.93, 10.82, -1.5));
    var finalColor = vec3<f32>(0.0);
    var alpha = 0.0;

    if (t <= maxDepth) {
        alpha = 1.0;
        // Normal from the distance-field gradient.
        let smallVec = vec3<f32>(0.005, 0.0, 0.0);
        let normalU = vec3<f32>(
            distAndMat.x - DistanceToObject(pos - smallVec.xyy).x,
            distAndMat.x - DistanceToObject(pos - smallVec.yxy).x,
            distAndMat.x - DistanceToObject(pos - smallVec.yyx).x);
        let normal = normalize(normalU);

        // Two-scale ambient occlusion by sampling the field along the normal.
        var ambientS = 1.0;
        ambientS = ambientS * sat(DistanceToObject(pos + normal * 0.1).x * 10.0);
        ambientS = ambientS * sat(DistanceToObject(pos + normal * 0.2).x * 5.0);
        ambientS = ambientS * sat(DistanceToObject(pos + normal * 0.4).x * 2.5);
        ambientS = ambientS * sat(DistanceToObject(pos + normal * 0.8).x * 1.25);
        var ambient = ambientS * sat(DistanceToObject(pos + normal * 1.6).x * 1.25 * 0.5);
        ambient = ambient * sat(DistanceToObject(pos + normal * 3.2).x * 1.25 * 0.25);
        ambient = ambient * sat(DistanceToObject(pos + normal * 6.4).x * 1.25 * 0.125);
        ambient = max(0.035, pow(ambient, 0.3));
        ambient = sat(ambient);

        // Reflection vector, traced for a soft self-shadow.
        var refl = normalize(reflect(rayVec, normal));
        var sunShadow = 1.0;
        var iter = 0.1;
        let nudgePos = pos + normal * 0.02;
        for (var j = 0; j < 40; j = j + 1) {
            let tempDist = DistanceToObject(nudgePos + refl * iter).x;
            sunShadow = sunShadow * sat(tempDist * 50.0);
            if (tempDist <= 0.0) {
                break;
            }
            iter = iter + max(0.0, tempDist);
            if (iter > 4.2) {
                break;
            }
        }
        sunShadow = sat(sunShadow);

        // Texture color: near-white shells, with the innermost core (material id 6) glowing hot pink.
        var texColor = vec3<f32>(0.85, 0.945 - distAndMat.y * 0.15, 0.93 + distAndMat.y * 0.35) * 0.951;
        if (distAndMat.y == 6.0) {
            texColor = vec3<f32>(0.91, 0.1, 0.41) * 10.5;
        }
        texColor = max(texColor, vec3<f32>(0.0)) * 0.25;

        // Hemisphere lights (sky + ground) modulated by ambient occlusion.
        var lightColor = vec3<f32>(0.1, 0.35, 0.95) * (normal.y * 0.5 + 0.5) * ambient * 0.2;
        lightColor = lightColor + vec3<f32>(1.0) * ((-normal.y) * 0.5 + 0.5) * ambient * 0.2;
        finalColor = texColor * lightColor;

        // Reflection environment map — most of the light.
        let refColor = GetEnvColor2(refl, sunDir) * sunShadow;
        finalColor = finalColor + refColor * 0.35 * ambient;

        // Distance fog toward a warm pink.
        finalColor = mix(vec3<f32>(2.0, 1.41, 1.41), finalColor, exp(-t * 0.0007));
    } else {
        // Missed the sphere → transparent, so only the orb draws over the game scene.
        finalColor = vec3<f32>(0.0);
        alpha = 0.0;
    }

    let outCol = sqrt(clamp(finalColor, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(outCol, alpha);
}
