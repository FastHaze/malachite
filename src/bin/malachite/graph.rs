//! The graph view (Ctrl+G): the vault as a ball of notes joined by their
//! links, turning slowly, to be turned, zoomed and clicked through.
//!
//! It is modelled on the graph of aicodingdictionary.com, read out of that
//! site's scripts, and the numbers in this file are that site's unless a
//! comment says otherwise: how the nodes drift and breathe, how the
//! neighbours of the note under the mouse lean towards it, how everything
//! else fades, how far things recede into the page with distance, how the
//! camera frames the ball, spins in, coasts and flies to a note, and how a
//! search shows only the notes it finds, gathered together.
//!
//! The site draws with shaders. Here every node and every point of every
//! edge is projected to the screen by hand and drawn with egui's painter, in
//! the order the site layers them: edges, nodes from the back forward, the
//! ring round the active note, the dots running along its links, the names.
//! Its film grain and (normally disabled) depth of field are left out.

use std::f32::consts::{PI, TAU};
use std::path::PathBuf;

use eframe::egui::{self, pos2, vec2, Color32, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use malachite::graph::{control, curve, Graph, Vec3};
use malachite::Note;

use crate::theme::Theme;
use crate::{Action, App};

/// Half the camera's vertical field of view, which is 50 degrees.
const HALF_FOV: f32 = 25.0 * PI / 180.0;
/// How near and how far the camera can be brought with the wheel.
const NEAREST: f32 = 70.0;
const FARTHEST: f32 = 1200.0;
/// How far from straight up or down the camera stops.
const POLE: f32 = 0.05;

/// How long the notes take to appear, the nearest to the middle first, and
/// how long each takes to grow once its turn has come.
const REVEAL: f32 = 2.4;
const GROW: f32 = 1.5;
/// The turn the camera arrives with when the graph first opens, and how long it takes.
const SPIN_IN: f32 = 4.2;
const SPIN_TURN: f32 = 6.6;
const SPIN_DIP: f32 = 0.6;
/// How fast the graph turns when left alone, in radians a second.
const IDLE_TURN: f32 = 0.045;

/// Where things start to fade into the page, and are fully faded, as
/// distances in front of and behind the middle of the ball.
const FADE_NEAR: f32 = 35.0;
const FADE_FAR: f32 = 200.0;
const NODE_FADE: f32 = 0.82;
const EDGE_FADE: f32 = 0.98;
const LABEL_FADE: f32 = 0.95;

/// Fog, which the site has only a trace of: past the middle of the ball
/// things thin out, and at its far side they are gone, so that what is drawn
/// is the nearer half and not all of it at once. Where it starts and where
/// it is complete, behind the middle, as parts of the ball's radius.
const FOG_FROM: f32 = 0.0;
const FOG_TO: f32 = 0.85;
/// A note this far gone into the fog cannot be pointed at.
const FOG_HIDES: f32 = 0.15;

/// While a search is on (`GraphView::search`), how much nearer the fade into
/// the page starts and how much of it is taken away: the notes found are
/// to be seen, near or far.
const SEARCH_NEAR: f32 = 90.0;
const SEARCH_CUT: f32 = 0.78;
/// How long what is looked for has to stay the same before the notes found
/// are gathered, so that they are not sent flying at every letter typed.
const GATHER_AFTER: f64 = 0.25;
/// How long a note takes to shrink away when a search leaves it out, or to
/// come back.
const SHOWN_EASE: f32 = 0.18;

/// How far out from the middle of a disc the mouse still has hold of its
/// note, in radii of the disc: a little way past its rim.
const REACH: f32 = 1.3;

/// How wide an edge is, in the graph's own units, and the least of a pixel
/// it is given credit for. The site's hairlines rely on its renderer
/// smoothing them; drawn here at their true strength they would vanish.
const EDGE_WIDTH: f32 = 0.15;
const EDGE_PRESENCE: f32 = 0.45;

/// The most dots that run along the active note's links, and how many each link gets.
const DOTS: usize = 320;
const DOTS_PER_EDGE: usize = 8;

/// Names are this much larger than on the site, where they are very small
/// at rest, and none is drawn below the size at which it is only a smudge.
const LABEL_SCALE: f32 = 1.5;
const SMALLEST_LABEL: f32 = 6.0;
const LARGEST_LABEL: f32 = 34.0;

/// From one of the sizes that names are laid out at to the next.
const LABEL_STEP: f32 = 1.2;

/// A size of text large enough to tell how wide the font's letters are for
/// each point of size: at the sizes names have, a size is rounded to whole
/// pixels before any glyph is made, which is a good part of it.
const MEASURED_AT: f32 = 96.0;

/// How many times a second the graph is drawn: while it is being used, and
/// for `SETTLE` seconds after, as things come to rest; while it is only being
/// watched, drifting; and while the window is not the one in front. The site
/// draws at whatever the screen does, which on a fast screen is a GPU kept
/// busy for a slow drift that looks no different at 30.
const BUSY_FPS: f32 = 60.0;
const WATCHED_FPS: f32 = 30.0;
const AWAY_FPS: f32 = 15.0;
const SETTLE: f64 = 3.0;

/// How long the graph drifts after it was last used before it comes to rest,
/// and how many seconds it takes to slow down to a stop (or to pick up
/// again). At rest nothing moves and nothing is drawn until the mouse or the
/// keyboard does something; a graph left on the screen then costs nothing,
/// where one that drifted for ever was drawn thirty times a second for as
/// long as it was there. The site drifts for ever.
const REST: f64 = 10.0;
const EASE_TO_REST: f32 = 1.5;

/// The size at which a name of this size is laid out: the nearest of a few.
///
/// Every size of text is a set of glyphs to make and keep, and as the graph
/// is zoomed a name goes through every size there is. So names are laid out
/// at a handful of sizes only, and each is then drawn at its own exact size
/// by stretching what was laid out (`stretched`), by a seventh at the very
/// most. That way a name grows and shrinks with the zoom as evenly as its
/// disc does, where text set at whole sizes went in jumps, a point at a time.
///
/// The sizes are whole numbers of pixels, `pixels_per_point` to the point,
/// so that they are the same ones whenever they are asked for.
fn laid_out_at(size: f32, pixels_per_point: f32) -> f32 {
    let step = ((size / SMALLEST_LABEL).ln() / LABEL_STEP.ln()).floor();
    let made_at = |step: f32| (SMALLEST_LABEL * LABEL_STEP.powf(step) * pixels_per_point).round().max(1.0) / pixels_per_point;
    let (below, above) = (made_at(step), made_at(step + 1.0));
    // Whichever has less scaling to do.
    if size * size <= below * above {
        below
    } else {
        above
    }
}

/// What a frame of the graph asks the app to do.
pub(crate) enum Outcome {
    Open(PathBuf),
    Close,
    /// Put the search away: a click on nothing while a search is on and a
    /// note has the focus lets go of both.
    StopSearching,
}

/// Where the camera is: turned `azimuth` round the upright axis and `polar`
/// down from it, `distance` away from the `target` it looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub azimuth: f32,
    pub polar: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Pose {
    /// The camera `distance` away from `target`, out in `direction` from it.
    fn looking_from(direction: Vec3, distance: f32, target: Vec3) -> Self {
        Self { azimuth: direction.x.atan2(direction.z), polar: direction.y.clamp(-1.0, 1.0).acos(), distance, target }
    }

    fn eye(&self) -> Vec3 {
        let (ring, height) = (self.polar.sin(), self.polar.cos());
        self.target + Vec3::new(ring * self.azimuth.sin(), height, ring * self.azimuth.cos()) * self.distance
    }

    /// Move a part of the way to `goal`.
    fn ease(&mut self, goal: &Pose, part: f32) {
        self.azimuth += (goal.azimuth - self.azimuth) * part;
        self.polar += (goal.polar - self.polar) * part;
        self.distance += (goal.distance - self.distance) * part;
        self.target = self.target.lerp(goal.target, part);
    }
}

/// The direction the camera looks at the ball from when nothing is focused.
fn home_direction() -> Vec3 {
    Vec3::new(130.0, 100.0, 540.0).normalized_or(Vec3::new(0.0, 0.0, 1.0))
}

/// How far back the camera stands to frame a ball of `radius` in a view of
/// `size`: back until the ball fits the narrower way, then a little closer,
/// so that it fills the view and its rim runs off the edge.
pub(crate) fn fit_distance(size: Vec2, radius: f32) -> f32 {
    let across = (size.x / size.y.max(1.0) * HALF_FOV.tan()).atan();
    let distance = (radius / HALF_FOV.min(across).sin()).clamp(1.4 * radius, FARTHEST);
    distance * if size.y >= size.x { 0.52 } else { 0.85 }
}

/// How far back the camera stands to frame the notes a search has gathered,
/// `reach` being how far from the middle they reach: all of them in view
/// with room round them, where the whole ball is let run off the edges.
fn gathered_distance(size: Vec2, reach: f32) -> f32 {
    let across = (size.x / size.y.max(1.0) * HALF_FOV.tan()).atan();
    let distance = ((reach.max(40.0) + 30.0) / HALF_FOV.min(across).sin()).clamp(120.0, FARTHEST);
    distance * if size.y >= size.x { 1.12 } else { 1.0 }
}

/// A camera pose worked out for a view on the screen: what turns a place in
/// the graph into a place in the window.
pub(crate) struct Lens {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    centre: Pos2,
    /// Pixels to a unit, one unit in front of the camera.
    scale: f32,
}

/// A point of the graph as the camera sees it.
#[derive(Clone, Copy)]
pub(crate) struct Seen {
    pub pos: Pos2,
    /// How far in front of the camera it is.
    pub depth: f32,
    /// Pixels to a unit of the graph at that depth.
    pub scale: f32,
}

impl Lens {
    pub(crate) fn new(pose: &Pose, view: Rect) -> Self {
        let eye = pose.eye();
        let forward = (pose.target - eye).normalized_or(Vec3::new(0.0, 0.0, -1.0));
        let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalized_or(Vec3::new(1.0, 0.0, 0.0));
        let up = right.cross(forward);
        Self { eye, right, up, forward, centre: view.center(), scale: view.height() / 2.0 / HALF_FOV.tan() }
    }

    /// Where `point` is on the screen, unless it is behind the camera or right upon it.
    pub(crate) fn project(&self, point: Vec3) -> Option<Seen> {
        let from_eye = point - self.eye;
        let depth = from_eye.dot(self.forward);
        if depth < 1.0 {
            return None;
        }
        let scale = self.scale / depth;
        let pos = self.centre + vec2(from_eye.dot(self.right), -from_eye.dot(self.up)) * scale;
        Some(Seen { pos, depth, scale })
    }
}

/// Fast at first and ever slower: how things arrive.
fn ease_out(t: f32) -> f32 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2f32.powf(-10.0 * t.max(0.0))
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A number between 0 and 1 that is always the same for the same `seed` and
/// has nothing visibly to do with it: what makes every node move a little
/// differently without anything being left to chance.
fn scatter(seed: f32) -> f32 {
    let value = 43_758.547 * (127.1 * seed + 311.7).sin();
    value - value.floor()
}

fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(channel(from.r(), to.r()), channel(from.g(), to.g()), channel(from.b(), to.b()))
}

/// Move `value` towards `goal`: unhurriedly when that makes it brighter, and
/// about twice as fast when it dims, so that what the mouse has left clears
/// out of the way of what it has come to.
fn attend(value: &mut f32, goal: f32, dt: f32) {
    let part = if goal > *value { 1.0 - 0.16f32.powf(dt) } else { 1.0 - 0.012f32.powf(dt) };
    *value += (goal - *value) * part;
}

/// How a node moves of its own accord, fixed when the graph is built.
struct Motion {
    /// Where in their cycles its three axes start, how far each drifts and how fast.
    phase: [f32; 3],
    reach: [f32; 3],
    rate: [f32; 3],
    /// Where in its cycle its slight swelling and shrinking starts.
    breath: f32,
    /// How stiffly it is drawn to an active neighbour, how far towards it, and how far off to one side.
    stiffness: f32,
    lean: f32,
    aside: Vec3,
    /// When, in the reveal, its turn to appear comes.
    turn: f32,
}

struct Edge {
    a: usize,
    b: usize,
    /// Where its bend is, from the point midway between its ends.
    bend: Vec3,
    /// How much wider than a plain edge it is drawn: links to hubs are heavier.
    weight: f32,
    /// Where along the curve it is drawn, from the rim of one disc to the
    /// rim of the other. Empty if the discs leave nothing between them.
    stops: Vec<f32>,
    /// What makes it shiver a little differently from the others.
    seed: f32,
    /// How strongly it is drawn now.
    strength: f32,
}

/// `text` made wider and taller by these two factors, from its top left
/// corner: what `Shape::transform` does to text, but for each direction on
/// its own. A name needs the two apart, because text is not laid out quite
/// as its size says. Glyphs are made at a whole number of pixels, and each
/// letter is then set a whole number of pixels after the last, so both the
/// height of a name and, separately, its width are a little off, by another
/// amount at every size. Stretched by what the size alone says, a name would
/// jump as it passed from one of the sizes it is laid out at to the next.
fn stretched(mut text: std::sync::Arc<egui::Galley>, wider: f32, taller: f32) -> std::sync::Arc<egui::Galley> {
    let by = vec2(wider, taller);
    let scaled = |rect: Rect| Rect::from_min_max((rect.min.to_vec2() * by).to_pos2(), (rect.max.to_vec2() * by).to_pos2());
    let galley = std::sync::Arc::make_mut(&mut text);
    for row in &mut galley.rows {
        row.visuals.mesh_bounds = scaled(row.visuals.mesh_bounds);
        for vertex in &mut row.visuals.mesh.vertices {
            vertex.pos = (vertex.pos.to_vec2() * by).to_pos2();
        }
    }
    galley.mesh_bounds = scaled(galley.mesh_bounds);
    galley.rect = scaled(galley.rect);
    text
}

/// A note's name, laid out and waiting to see whether there is room for it.
struct Name {
    node: usize,
    corner: Pos2,
    text: std::sync::Arc<egui::Galley>,
    color: Color32,
    opacity: f32,
    /// Which of two names that would overlap is written.
    rank: f32,
}

/// One of the dots running along the links of the active note.
#[derive(Clone, Copy, Default)]
struct Dot {
    edge: usize,
    along: f32,
    speed: f32,
    /// Running from the edge's second end to its first, so as to leave the active note.
    backward: bool,
    size: f32,
    on: bool,
}

pub(crate) struct GraphView {
    /// The `App::version` this was built from.
    pub(crate) version: u64,
    graph: Graph,
    paths: Vec<PathBuf>,
    titles: Vec<String>,
    radius: Vec<f32>,
    rest: Vec<Vec3>,
    /// The radius that holds nearly all of the ball: a few notes always lie
    /// well outside the rest, and are not what it is framed by.
    body: f32,
    motion: Vec<Motion>,
    /// How far each node has been drawn from its place by an active neighbour, and how fast it is moving.
    lean: Vec<Vec3>,
    speed: Vec<Vec3>,
    /// Where each node is this frame, and how big.
    pub(crate) at: Vec<Vec3>,
    size: Vec<f32>,
    /// How much each node stands out: 1 as usual, more when active, less when something else is.
    standing: Vec<f32>,
    /// How far each name has been brought forward by the mouse, while another note has the focus.
    raised: Vec<f32>,
    /// How much of each name is showing: a name gives way to a more
    /// important one it would be written across.
    named: Vec<f32>,
    edges: Vec<Edge>,
    dots: Vec<Dot>,
    /// The note the dots were last set running for, and the search they were set running in.
    dots_for: (Option<usize>, u32),

    /// The notes a search has found, the likeliest first, while one is on (`search`).
    found: Option<Vec<usize>>,
    /// The notes drawn, while not all of them are (`choose`): the note in
    /// focus and its neighbours, or what a search has found and theirs,
    /// gathered about the middle.
    drawn: Option<Vec<usize>>,
    /// Whether each note is drawn, and how much of each is showing, which follows that.
    matched: Vec<bool>,
    pub(crate) shown: Vec<f32>,
    /// Where each note drawn is gathered to, as the way from where it rests;
    /// how far from the middle the farthest of them reaches; and when they are
    /// to be gathered, while that is still to come.
    gathered: Vec<Vec3>,
    gathered_reach: f32,
    gather_at: Option<f64>,
    /// How far the look of the graph has gone over to that of a few notes
    /// gathered, from 0 to 1.
    searching: f32,
    /// One more for every new search, or new notes found.
    searches: u32,
    random: u32,

    pub(crate) camera: Pose,
    goal: Pose,
    /// Whether the camera has been put in its place for the view's size yet.
    placed: bool,
    /// Whether the opening spin is still playing.
    spinning: bool,
    /// How fast a drag last turned the camera, round and down, for it to coast on.
    turning: Vec2,
    /// When the camera was last moved by hand, and until when it is left to finish a flight.
    touched: f64,
    settling: f64,

    opened: f64,
    /// When the last frame was drawn, and when the mouse or the keyboard
    /// last did anything here (see `frames_per_second`).
    drawn_at: f64,
    stirred: f64,
    /// How lively the graph is, from 1 while it is in use to 0 at rest; and
    /// how far its own clock has got, which runs at a speed that follows it. The drift is
    /// worked out from this clock, so that it stops where it is and goes on
    /// from there, and does not jump to where the time of day would put it.
    lively: f32,
    drifted: f64,
    pub(crate) hovered: Option<usize>,
    pub(crate) focused: Option<usize>,
    focused_at: f64,
    /// The ring: which note it is round, how visible it is and how far it has closed in.
    ringed: Option<usize>,
    ring_shown: f32,
    ring_settled: f32,
    /// Where each node was drawn, for tests to aim at, and whether it was
    /// too far into the fog to be pointed at.
    #[cfg(test)]
    pub(crate) seen: Vec<Option<Pos2>>,
    #[cfg(test)]
    pub(crate) fogged: Vec<bool>,
}

impl GraphView {
    /// Lay the notes out. `before` is the view this replaces, if the notes
    /// have changed under one: the camera stays where it was, nothing is
    /// revealed a second time, and the focus stays on its note.
    pub(crate) fn build(notes: &[Note], version: u64, now: f64, before: Option<GraphView>) -> Self {
        let graph = Graph::build(notes);
        let paths: Vec<PathBuf> = notes.iter().map(|note| note.path.clone()).collect();
        if let Some(mut before) = before {
            if before.paths == paths && before.graph.edges == graph.edges {
                // The same picture: only what the notes are called can have changed.
                before.titles = notes.iter().map(|note| note.title.to_uppercase()).collect();
                before.version = version;
                return before;
            }
            let mut view = Self::build(notes, version, now, None);
            let kept = |node: Option<usize>| node.and_then(|node| paths.iter().position(|path| *path == before.paths[node]));
            (view.camera, view.goal, view.placed, view.spinning) = (before.camera, before.goal, before.placed, before.spinning);
            (view.opened, view.touched, view.settling) = (before.opened, before.touched, before.settling);
            (view.stirred, view.lively, view.drifted) = (before.stirred, before.lively, before.drifted);
            (view.focused, view.focused_at) = (kept(before.focused), before.focused_at);
            if before.focused.is_some() && view.focused.is_none() {
                // The note it was on has gone: back to the whole ball.
                view.placed = false;
            }
            // The search is given again by the next call of `search`.
            if view.focused.is_some() {
                view.choose();
                view.gather_at = Some(now);
            }
            return view;
        }

        let count = notes.len();
        let rest = graph.layout();
        let radius: Vec<f32> = (0..count).map(|node| graph.radius(node)).collect();
        let mut reach: Vec<f32> = rest.iter().map(|at| at.length()).collect();
        reach.sort_by(f32::total_cmp);
        let extent = reach.last().copied().unwrap_or(0.0).max(1.0);
        let body = reach.get(reach.len() * 19 / 20).copied().unwrap_or(extent).max(1.0);
        let motion = (0..count)
            .map(|node| {
                let n = node as f32;
                let turn = 2.399_96 * n;
                let axis = |k: usize| 3.0 * n + k as f32;
                Motion {
                    phase: std::array::from_fn(|k| turn + 6.283 * scatter(axis(k))),
                    reach: std::array::from_fn(|k| 3.0 * (0.45 + 1.1 * scatter(axis(k) + 7.1))),
                    rate: std::array::from_fn(|k| 0.19 * (0.6 + 1.1 * scatter(axis(k) + 13.3))),
                    breath: turn,
                    stiffness: 42.0 * (0.45 + 1.4 * scatter(n + 0.5)),
                    lean: 0.24 * (0.55 + 0.95 * scatter(n + 4.2)),
                    aside: Vec3::new(scatter(n + 21.1) - 0.5, scatter(n + 31.7) - 0.5, scatter(n + 41.3) - 0.5) * 8.0,
                    turn: rest[node].length() / extent * 0.9,
                }
            })
            .collect();

        // A crowded graph gets plainer edges: fewer points to each, then straight lines.
        let stops = match graph.edges.len() {
            0..=600 => 16,
            601..=2500 => 8,
            _ => 2,
        };
        let edges = graph
            .edges
            .iter()
            .map(|&(a, b)| {
                let bend = control(rest[a], rest[b]) - (rest[a] + rest[b]) * 0.5;
                let most_linked = graph.nodes[a].neighbors.len().max(graph.nodes[b].neighbors.len()) as f32;
                let seed = (a as f32 * 12.9898 + b as f32 * 78.233).sin() * 43_758.547;
                Edge {
                    a,
                    b,
                    bend,
                    weight: 0.75 + 0.45 * (most_linked / 12.0).min(1.0),
                    stops: between_rims(rest[a], rest[b], bend, radius[a], radius[b], stops),
                    seed: seed - seed.floor(),
                    strength: 0.08,
                }
            })
            .collect();

        Self {
            version,
            paths,
            titles: notes.iter().map(|note| note.title.to_uppercase()).collect(),
            body,
            motion,
            lean: vec![Vec3::ZERO; count],
            speed: vec![Vec3::ZERO; count],
            at: rest.clone(),
            size: radius.clone(),
            standing: vec![1.0; count],
            raised: vec![0.0; count],
            named: vec![1.0; count],
            radius,
            rest,
            edges,
            dots: vec![Dot::default(); DOTS],
            dots_for: (None, 0),
            found: None,
            drawn: None,
            matched: vec![true; count],
            shown: vec![1.0; count],
            gathered: vec![Vec3::ZERO; count],
            gathered_reach: 0.0,
            gather_at: None,
            searching: 0.0,
            searches: 0,
            random: 0x9E37_79B9,
            graph,
            camera: Pose::looking_from(home_direction(), 600.0, Vec3::ZERO),
            goal: Pose::looking_from(home_direction(), 600.0, Vec3::ZERO),
            placed: false,
            spinning: true,
            turning: Vec2::ZERO,
            touched: now,
            settling: now,
            opened: now,
            drawn_at: now,
            stirred: now,
            lively: 1.0,
            drifted: 0.0,
            hovered: None,
            focused: None,
            focused_at: now,
            ringed: None,
            ring_shown: 0.0,
            ring_settled: 0.0,
            #[cfg(test)]
            seen: Vec::new(),
            #[cfg(test)]
            fogged: Vec::new(),
        }
    }

    /// How many lines there are between notes.
    #[cfg(test)]
    pub(crate) fn links(&self) -> usize {
        self.edges.len()
    }

    /// The file of the note that has the focus.
    fn focused_path(&self) -> Option<&PathBuf> {
        self.focused.map(|node| &self.paths[node])
    }

    /// The note that has the attention: the focused one, or else the one under the mouse.
    fn active(&self) -> Option<usize> {
        self.focused.or(self.hovered)
    }

    /// The camera's place for the whole ball in a view of `size`.
    fn home(&self, size: Vec2) -> Pose {
        // Framed by the body of the ball, so that it fills the view and the
        // outliers reach to its edges, as the site's ball does.
        Pose::looking_from(home_direction(), fit_distance(size, self.body * 1.1 + 20.0), Vec3::ZERO)
    }

    /// Give `node` the focus and fly the camera to it, or with `None` back
    /// out to the whole ball, or to the notes found while a search is on.
    ///
    /// A note in focus is shown with the notes it is linked with and nothing
    /// else, gathered together as the notes a search finds are (`choose`).
    pub(crate) fn focus(&mut self, node: Option<usize>, size: Vec2, now: f64) {
        if node == self.focused {
            return;
        }
        self.focused = node;
        self.focused_at = now;
        self.spinning = false;
        self.turning = Vec2::ZERO;
        self.settling = now + 1.1;
        // At once: a click is answered there and then, where what is typed
        // is waited on.
        self.choose();
        self.gather(now);
        self.aim(size);
    }

    /// Send the camera to what is to be looked at now: the note that has the
    /// focus, else the notes found, else the whole ball. Notes still to be
    /// gathered are not framed yet: the camera waits for them where it is.
    fn aim(&mut self, size: Vec2) {
        if self.drawn.is_some() && self.gather_at.is_some() {
            return;
        }
        let mut goal = match (self.focused, &self.drawn) {
            // The note in focus is looked at where it has been gathered to,
            // from where the camera is, and from far enough back to see the
            // notes round it.
            (Some(node), Some(drawn)) => {
                let at = self.rest[node] + self.gathered[node];
                let reach = drawn.iter().map(|&other| (self.rest[other] + self.gathered[other]).distance(at) + self.radius[other]).fold(0.0, f32::max);
                let near = (125.0 + 8.0 * self.radius[node]) * if size.y >= size.x { 1.12 } else { 1.0 };
                // Going to a note must still mean going nearer, in a small vault too.
                let distance = near.max(gathered_distance(size, reach + 16.0)).min(0.75 * self.home(size).distance);
                Pose { distance, target: at, ..self.goal }
            }
            // The notes found are framed from where the camera already is.
            (None, Some(_)) => Pose { distance: gathered_distance(size, self.gathered_reach), target: Vec3::ZERO, ..self.goal },
            _ => self.home(size),
        };
        // Round the short way, not back through every turn made so far.
        goal.azimuth += ((self.camera.azimuth - goal.azimuth) / TAU).round() * TAU;
        self.goal = goal;
    }

    /// Draw only `found` of the notes, by their places in the graph, and the
    /// notes linked with them, gathered about the middle where they can be
    /// seen together; or with `None`, or none found, all of them where they
    /// rest again. Called every frame with what the find bar has found; only
    /// a change does anything.
    ///
    /// The site draws the notes found alone. But it finds a note by
    /// everything written in it, and a note's neighbours mostly speak of it
    /// and are found with it; found by its name alone, a note came out with
    /// none of what it is linked with.
    ///
    /// The notes left out shrink away at once, and the ones found are
    /// gathered a moment later (`GATHER_AFTER`), with the camera framing
    /// them, as on the site. Starting a search lets go of the note in focus,
    /// as there; and so does a search that no longer finds it, which there
    /// leaves the focus on a note that is not drawn.
    pub(crate) fn search(&mut self, found: Option<&[usize]>, size: Vec2, now: f64) {
        let found = found.filter(|found| !found.is_empty());
        if found == self.found.as_deref() {
            return;
        }
        let started = self.found.is_none();
        self.found = found.map(<[usize]>::to_vec);
        self.searches = self.searches.wrapping_add(1);
        self.stirred = now;
        self.settling = self.settling.max(now + 1.1);
        if let (Some(found), Some(focused)) = (found, self.focused) {
            if started || !found.contains(&focused) {
                self.focus(None, size, now);
                return;
            }
        }
        if self.choose() {
            match self.drawn {
                Some(_) => self.gather_at = Some(now + GATHER_AFTER),
                // Back out to the whole ball from where the camera is, as
                // the site does, rather than from where it first looked.
                None => self.goal = Pose { distance: self.home(size).distance, target: Vec3::ZERO, ..self.goal },
            }
        }
    }

    /// Work out which notes are to be drawn: the note in focus and the notes
    /// linked with it; else the notes a search has found and the notes linked
    /// with those; else all of them. Whether that is not what it was.
    ///
    /// The notes are not gathered here, only sent back to where they rest
    /// until they are (`gather`).
    fn choose(&mut self) -> bool {
        let chosen: Option<Vec<usize>> = match (self.focused, &self.found) {
            (Some(node), _) => Some(std::iter::once(node).chain(self.graph.nodes[node].neighbors.iter().copied()).collect()),
            (None, Some(found)) => {
                let mut chosen = found.clone();
                let mut taken: std::collections::HashSet<usize> = found.iter().copied().collect();
                for &node in found {
                    chosen.extend(self.graph.nodes[node].neighbors.iter().copied().filter(|&neighbor| taken.insert(neighbor)));
                }
                Some(chosen)
            }
            (None, None) => None,
        };
        if chosen == self.drawn {
            return false;
        }
        self.matched.fill(chosen.is_none());
        for &node in chosen.iter().flatten() {
            self.matched[node] = true;
        }
        self.drawn = chosen;
        self.gathered.fill(Vec3::ZERO);
        (self.gather_at, self.gathered_reach) = (None, 0.0);
        true
    }

    /// Gather the notes to be drawn, now.
    fn gather(&mut self, now: f64) {
        self.gather_at = None;
        let Some(drawn) = &self.drawn else { return };
        let gathered = self.graph.gather(&self.rest, drawn);
        for (&node, &at) in drawn.iter().zip(&gathered) {
            self.gathered[node] = at - self.rest[node];
        }
        let reach = drawn.iter().zip(&gathered).map(|(&node, at)| at.length() + self.radius[node]).fold(0.0, f32::max);
        self.gathered_reach = reach + 16.0;
        self.settling = self.settling.max(now + 1.1);
    }

    fn next_random(&mut self) -> f32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        (self.random >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Move the camera for this frame: by the mouse, by the opening spin, by
    /// coasting on after a drag, or by the slow turn of a graph left alone.
    fn steer(&mut self, view: Rect, response: &egui::Response, scroll: f32, now: f64, dt: f32) {
        if !self.placed {
            self.goal = match self.focused {
                Some(_) => self.goal,
                None => self.home(view.size()),
            };
            self.camera = self.goal;
            self.placed = true;
        }
        let dragging = response.dragged();
        if dragging {
            self.spinning = false;
            let per_pixel = TAU * 0.6 / view.height().max(1.0);
            let turn = response.drag_delta() * -per_pixel;
            self.goal.azimuth += turn.x;
            self.goal.polar = (self.goal.polar + turn.y).clamp(POLE, PI - POLE);
            let limit = |speed: f32| speed.clamp(-2.4, 2.4);
            self.turning = vec2(limit(turn.x / dt.max(1.0 / 120.0)), limit(turn.y / dt.max(1.0 / 120.0)));
            self.touched = now;
        }
        if scroll != 0.0 {
            self.goal.distance = (self.goal.distance * (-scroll * 0.002).exp()).clamp(NEAREST, FARTHEST);
        }

        if self.spinning {
            let spun = ((now - self.opened) as f32 / SPIN_IN).min(1.0);
            let left = 1.0 - ease_out(spun);
            let home = self.home(view.size());
            self.goal = Pose { azimuth: home.azimuth - SPIN_TURN * left, polar: home.polar + SPIN_DIP * left, ..home };
            self.camera = self.goal;
            self.touched = now;
            self.spinning = spun < 1.0;
            return;
        }
        if !dragging && self.active().is_none() && now >= self.settling {
            if self.turning.x.abs() > 0.05 || self.turning.y.abs() > 0.05 {
                self.goal.azimuth += self.turning.x * dt;
                self.goal.polar = (self.goal.polar + self.turning.y * dt).clamp(POLE, PI - POLE);
                self.turning *= (-dt / 0.6).exp();
                self.touched = now;
            } else {
                let idle = (((now - self.touched) as f32 - 0.5) / 1.6).clamp(0.0, 1.0);
                self.goal.azimuth += IDLE_TURN * dt * self.speed() * idle * idle * (3.0 - 2.0 * idle);
            }
        }
        // A drag is followed closely; a flight to a note takes its time.
        let follow = if dragging { 0.035 } else { 0.16 };
        self.camera.ease(&self.goal, 1.0 - (-dt / follow).exp());
    }

    /// Work out where every node is this frame and how big.
    fn animate(&mut self, now: f64, dt: f32) {
        let clock = (now - self.opened) as f32;
        // The drift's own clock, which stops when the graph is at rest.
        let drift_clock = self.drifted as f32;
        let revealed = clock >= REVEAL;
        let active = self.active();
        // The note that is active only because the mouse is on it.
        let pointed = self.hovered.filter(|_| self.focused.is_none());
        for node in 0..self.rest.len() {
            let motion = &self.motion[node];
            let grown = if revealed { 1.0 } else { ease_out(((clock - motion.turn) / GROW).clamp(0.0, 1.0)) };
            let sway = |k: usize| {
                let (rate, phase) = (drift_clock * motion.rate[k], motion.phase[k]);
                motion.reach[k] * (0.7 * (rate + phase).sin() + 0.3 * (rate * 2.3 + 1.7 * phase).sin())
            };
            let drift = Vec3::new(sway(0), sway(1), sway(2));
            // A note left out shrinks away, and one taken back comes back.
            let wanted = if self.matched[node] { 1.0 } else { 0.0 };
            self.shown[node] += (wanted - self.shown[node]) * (1.0 - (-dt.min(0.05) / SHOWN_EASE).exp());
            // While only some notes are drawn, each goes to where it is
            // gathered to, on the same spring; otherwise a neighbour of the
            // active note leans towards it.
            let towards = match active {
                _ if self.drawn.is_some() => self.gathered[node],
                Some(active) if self.graph.nodes[active].neighbors.binary_search(&node).is_ok() => {
                    (self.rest[active] - self.rest[node]) * motion.lean + motion.aside
                }
                _ => Vec3::ZERO,
            };
            if pointed == Some(node) {
                // Under the mouse a note stands still, wherever it had leant
                // to. One that went back to its place on being pointed at
                // would go out from under the pointer.
                self.speed[node] = Vec3::ZERO;
            } else {
                let pull = (towards - self.lean[node]) * motion.stiffness - self.speed[node] * 17.0;
                self.speed[node] += pull * dt;
                self.lean[node] += self.speed[node] * dt;
            }
            self.at[node] = (self.rest[node] + drift) * grown + self.lean[node];
            let breath = 1.0 + 0.012 * (0.5 * drift_clock + motion.breath).sin();
            self.size[node] = (self.radius[node] * grown * breath * self.shown[node]).max(1e-4);
        }
    }

    /// The note the pointer is on, given the one it was on a moment ago and
    /// whether the pointer has `moved` since.
    ///
    /// A disc is pointed at from a little way outside it too (`REACH`). Where
    /// discs overlap the pointer is on the one in front, as it looks to be,
    /// and with no disc right under it, on the nearest one within reach.
    ///
    /// But the note it is on already keeps it, for as long as it is within
    /// that note's reach. A pointer that is held still stays with its note
    /// whatever else comes under it: only the hand takes it elsewhere. One
    /// that is moving goes over to another disc it has come onto, if that
    /// disc is in front of this note's, or is any disc at all once the
    /// pointer is off this note's own and only in the room round it.
    ///
    /// It did once go to the nearest note every time. But a note's
    /// neighbours lean towards it while the pointer is on it, so the nearest
    /// at one moment is not the nearest at the next, and a pointer held still
    /// between a note and one behind it went back and forth between the two
    /// several times a second.
    fn pick(&self, lens: &Lens, seen: &[Option<Seen>], clear: &[f32], pointer: Pos2, held: Option<usize>, moved: bool) -> Option<usize> {
        // How far the pointer is from the middle of a note's disc, in radii
        // of the disc, for a note that can be pointed at at all.
        let away = |node: usize| {
            // Not one in the fog, nor one a search has left out or is leaving out.
            let seen = seen.get(node).copied().flatten().filter(|_| clear[node] >= FOG_HIDES && self.shown[node] >= 0.5)?;
            Some(seen.pos.distance(pointer) / (self.radius[node] * seen.scale).max(1e-3))
        };
        // Which of two notes is in front goes by where they rest, not by
        // where they have leant to, which changes with what the pointer is on.
        let depth = |node: usize| (self.rest[node] - lens.eye).dot(lens.forward);
        let held = held.and_then(|held| Some((held, away(held).filter(|away| *away < REACH)?)));
        if let Some((held, _)) = held.filter(|_| !moved) {
            return Some(held);
        }
        let under = (0..seen.len()).filter(|&node| away(node).is_some_and(|away| away < 1.0));
        let front = under.min_by(|&a, &b| depth(a).total_cmp(&depth(b)));
        if let Some((held, off)) = held {
            let taken = front.filter(|&front| front != held && (off >= 1.0 || depth(front) < depth(held)));
            return Some(taken.unwrap_or(held));
        }
        let within = (0..seen.len()).filter_map(|node| Some((node, away(node).filter(|away| *away < REACH)?)));
        front.or_else(|| within.min_by(|a, b| a.1.total_cmp(&b.1)).map(|(node, _)| node))
    }

    /// Set dots running along the links of the active note, when that changes.
    fn release_dots(&mut self) {
        let active = self.active();
        if (active, self.searches) == self.dots_for {
            return;
        }
        self.dots_for = (active, self.searches);
        // Not along a link to a note a search has left out.
        let other_end = |edge: &Edge| if edge.a == active? { Some(edge.b) } else { (edge.b == active?).then_some(edge.a) };
        let links: Vec<usize> = (0..self.edges.len()).filter(|&edge| other_end(&self.edges[edge]).is_some_and(|other| self.matched[other])).collect();
        let wanted = (DOTS_PER_EDGE * links.len()).min(DOTS);
        for index in 0..DOTS {
            self.dots[index] = match (index < wanted, active) {
                (true, Some(active)) => {
                    let edge = links[index % links.len()];
                    let (along, speed, size) = (self.next_random(), self.next_random(), self.next_random());
                    Dot { edge, along, speed: 0.1 + 0.1 * speed, backward: self.edges[edge].a != active, size: 2.4 + 1.4 * size, on: true }
                }
                _ => Dot::default(),
            };
        }
    }

    /// Draw one frame into `view` and take the mouse and, if `keys`, the keyboard.
    /// How often to draw the graph from here on: at the full pace while it is
    /// being used or anything is still moving because of that (a flight to a
    /// note, the opening spin, a drag coasting to a stop, names and links
    /// coming forward), at a slower one while it only drifts, and slower
    /// again while the window is behind another.
    /// None once it has come to rest: it is then drawn again when there is
    /// something to answer.
    pub(crate) fn frames_per_second(&self, now: f64, window_in_front: bool) -> Option<f32> {
        if self.lively == 0.0 {
            return None;
        }
        Some(match (window_in_front, self.busy(now)) {
            (_, true) => BUSY_FPS,
            (true, false) => WATCHED_FPS,
            (false, false) => AWAY_FPS,
        })
    }

    /// How fast the graph's own clock runs, from `lively`: easing out of
    /// rest and into it, rather than starting and stopping at a stroke.
    fn speed(&self) -> f32 {
        self.lively * self.lively * (3.0 - 2.0 * self.lively)
    }

    /// Whether anything is still moving because the graph was used: the
    /// opening spin, a flight to a note, a drag coasting, names and links
    /// coming forward or going back, notes found waiting to be gathered.
    fn busy(&self, now: f64) -> bool {
        self.spinning || now < self.settling || now - self.stirred < SETTLE || now - self.focused_at < SETTLE || self.gather_at.is_some()
    }

    pub(crate) fn frame(&mut self, ui: &egui::Ui, view: Rect, response: &egui::Response, theme: &Theme, keys: bool) -> Option<Outcome> {
        let (now, scroll, stirring) = ui.input(|input| {
            let stirring = !input.events.is_empty() || input.pointer.delta() != Vec2::ZERO || input.smooth_scroll_delta != Vec2::ZERO;
            (input.time, input.smooth_scroll_delta.y, stirring)
        });
        // The time since the last frame, measured here: egui's own takes a
        // frame asked for with a delay to be one sixtieth of a second, which
        // at the slower paces would slow everything down with it.
        let dt = ((now - self.drawn_at) as f32).clamp(0.0, 0.1);
        self.drawn_at = now;
        if stirring {
            self.stirred = now;
        }
        // Towards rest, or back, at an even rate; the speed eases in and out of it.
        let wanted = if self.busy(now) || now - self.stirred < REST { 1.0 } else { 0.0 };
        let step = dt / EASE_TO_REST;
        self.lively = if wanted > self.lively { (self.lively + step).min(1.0) } else { (self.lively - step).max(0.0) };
        self.drifted += f64::from(dt * self.speed());
        if self.gather_at.is_some_and(|at| now >= at) {
            self.gather(now);
            self.aim(view.size());
        }
        let searched = if self.drawn.is_some() { 1.0 } else { 0.0 };
        self.searching += (searched - self.searching) * (1.0 - 9e-4f32.powf(dt));
        // While only some notes are drawn, edges run straight, and things
        // fade into the page less and only further off.
        let (curl, searching) = (1.0 - self.searching, self.searching);
        let fade_near = FADE_NEAR + SEARCH_NEAR * searching;
        let fade_cut = 1.0 - SEARCH_CUT * searching;
        let scroll = if response.hovered() { scroll } else { 0.0 };
        self.steer(view, response, scroll, now, dt);
        self.animate(now, dt);

        let lens = Lens::new(&self.camera, view);
        let seen: Vec<Option<Seen>> = self.at.iter().map(|&at| lens.project(at)).collect();
        #[cfg(test)]
        {
            self.seen = seen.iter().map(|seen| seen.map(|seen| seen.pos)).collect();
        }

        // How far the camera is from the middle of the ball: what "near" and "far" are measured from.
        let middle = lens.eye.length();
        // How much of a thing at this depth the fog leaves.
        let (fog_from, fog_to) = (middle + FOG_FROM * self.body, middle + FOG_TO * self.body);
        let through_fog = move |depth: f32| 1.0 - smoothstep(fog_from, fog_to, depth);
        // The active note and its neighbours are lifted out of it: they are what is being looked at.
        let clear: Vec<f32> = (0..seen.len())
            .map(|node| {
                // And so are the notes gathered.
                let lifted = ((self.standing[node] - 1.0) / 0.3).clamp(0.0, 1.0).max(searching * self.shown[node]);
                seen[node].map_or(0.0, |at| through_fog(at.depth).max(lifted))
            })
            .collect();

        #[cfg(test)]
        {
            self.fogged = clear.iter().map(|&clear| clear < FOG_HIDES).collect();
        }

        // The mouse: what it is over, and what a click on that does.
        let mut outcome = None;
        let moved = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
        self.hovered = match response.hover_pos() {
            Some(pointer) if !response.dragged() => self.pick(&lens, &seen, &clear, pointer, self.hovered, moved),
            _ => None,
        };
        if self.hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            let clicked = response.interact_pointer_pos().and_then(|pointer| self.pick(&lens, &seen, &clear, pointer, self.hovered, moved));
            match clicked {
                Some(node) if self.focused == Some(node) => outcome = Some(Outcome::Open(self.paths[node].clone())),
                // A click on nothing, while a search is on and a note is in
                // focus, puts both away, as on the site.
                None if self.focused.is_some() && self.found.is_some() => {
                    self.focus(None, view.size(), now);
                    outcome = Some(Outcome::StopSearching);
                }
                clicked => self.focus(clicked, view.size(), now),
            }
        }
        if keys {
            let (enter, escape) = ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
            match (self.focused, enter, escape) {
                (Some(node), true, _) => outcome = Some(Outcome::Open(self.paths[node].clone())),
                (Some(_), _, true) => self.focus(None, view.size(), now),
                (None, _, true) => outcome = Some(Outcome::Close),
                _ => {}
            }
        }
        self.release_dots();

        let painter = ui.painter_at(view);
        let (paper, ink, accent) = (theme.page, theme.text, theme.accent);
        let clock = (now - self.opened) as f32;
        let reveal = (clock / REVEAL).min(1.0);
        let (active, focused) = (self.active(), self.focused);
        let is_neighbor = |of: usize, node: usize| self.graph.nodes[of].neighbors.binary_search(&node).is_ok();

        // Edges, the faintest layer, under everything.
        let edges_in = if reveal >= 1.0 { 1.0 } else { ((reveal - 0.45) / 0.55).clamp(0.0, 1.0) };
        let plucked = (now - self.focused_at) as f32;
        let mut shapes = Vec::new();
        let mut points: Vec<Seen> = Vec::new();
        for edge in &mut self.edges {
            let joins = |node: Option<usize>| node.is_some_and(|node| edge.a == node || edge.b == node);
            // While only some notes are drawn, so are only the links between them.
            let wanted = match active {
                _ if self.drawn.is_some() && !(self.matched[edge.a] && self.matched[edge.b]) => 0.0,
                _ if self.drawn.is_some() && !joins(active) => 0.28,
                Some(_) if joins(active) => 0.6,
                Some(_) => 0.022,
                None => 0.08,
            };
            attend(&mut edge.strength, wanted, dt);
            let strength = edge.strength * edges_in;
            if strength < 0.004 || edge.stops.is_empty() {
                continue;
            }
            let (a, b) = (self.at[edge.a], self.at[edge.b]);
            let bend = (a + b) * 0.5 + edge.bend * curl;
            points.clear();
            points.extend(edge.stops.iter().filter_map(|&stop| lens.project(curve(a, bend, b, stop))));
            if points.len() < edge.stops.len() {
                continue;
            }
            // A link of the note that has just been focused is plucked like
            // a string: a wave runs out along it from that note and dies away.
            let shiver = joins(focused) && plucked < 6.0;
            let length = a.distance(b);
            let from_b = focused == Some(edge.b);
            for index in 0..points.len() - 1 {
                let (mut from, mut to) = (points[index], points[index + 1]);
                if shiver {
                    let across = (to.pos - from.pos).rot90().normalized();
                    let sway = |stop: f32| {
                        let along = if from_b { 1.0 - stop } else { stop };
                        let fading = (-plucked * (1.5 + edge.seed * 0.8)).exp();
                        let front = 1.0 - smoothstep(plucked * (2.6 + edge.seed * 1.2), plucked * (2.6 + edge.seed * 1.2) + 0.5, along);
                        let wave = ((along * (0.8 + edge.seed * 0.8) - plucked * 0.6 + edge.seed) * TAU).sin();
                        fading * front * (along * PI).sin() * wave * length * (0.022 + edge.seed * 0.012)
                    };
                    from.pos += across * (sway(edge.stops[index]) * from.scale);
                    to.pos += across * (sway(edge.stops[index + 1]) * to.scale);
                }
                let depth = (from.depth + to.depth) / 2.0;
                let fade = 1.0 - smoothstep(middle - FADE_NEAR / 2.0, middle + FADE_FAR / 2.0, depth) * EDGE_FADE;
                let width = EDGE_WIDTH * edge.weight * (from.scale + to.scale) / 2.0;
                let fog = if joins(active) { 1.0 } else { through_fog(depth).max(searching * self.shown[edge.a].min(self.shown[edge.b])) };
                let alpha = strength * fade * fade * fog * width.clamp(EDGE_PRESENCE, 1.0);
                if alpha >= 0.004 {
                    shapes.push(Shape::line_segment([from.pos, to.pos], Stroke::new(width.max(1.0), ink.gamma_multiply(alpha))));
                }
            }
        }
        painter.extend(shapes);

        // Nodes, from the back forward so that nearer discs cover further ones.
        let mut order: Vec<usize> = (0..seen.len()).filter(|&node| seen[node].is_some()).collect();
        // The discs drawn, for the names to see whether there is one under them.
        let mut discs: Vec<(Pos2, f32)> = Vec::new();
        order.sort_by(|&a, &b| seen[b].map(|s| s.depth).partial_cmp(&seen[a].map(|s| s.depth)).unwrap_or(std::cmp::Ordering::Equal));
        for &node in &order {
            let Some(at) = seen[node] else { continue };
            let wanted = match active {
                None => 1.0,
                Some(active) if active == node => 2.0,
                Some(active) if is_neighbor(active, node) => 1.3,
                Some(_) if focused.is_some() => 0.035,
                Some(_) => 0.12,
            };
            attend(&mut self.standing[node], wanted, dt);
            let standing = self.standing[node];
            let radius = self.size[node] * at.scale;
            if radius < 0.3 || clear[node] < 0.02 {
                continue;
            }
            let mut color = mix(ink, accent, standing - 1.0);
            // Discs behind the front of the ball lift a little towards the
            // page, so that one in front of another can be told from it.
            color = mix(color, paper, ((at.depth - (middle - 160.0)) / 200.0).clamp(0.0, 1.0) * 0.22);
            let fade = 1.0 - smoothstep(middle - fade_near, middle + FADE_FAR, at.depth) * NODE_FADE * fade_cut;
            let fade = fade * (0.2 + 0.8 * standing.clamp(0.0, 1.0));
            painter.circle_filled(at.pos, radius, mix(paper, color, fade * clear[node]));
            discs.push((at.pos, radius));
        }

        // The ring round the active note, wide at first and closing in.
        let ring_part = |seconds: f32| 1.0 - (-dt / seconds).exp();
        if active.is_some() && active != self.ringed {
            self.ringed = active;
            self.ring_settled = 0.0;
        }
        let wanted = if active.is_some() { 0.95 } else { 0.0 };
        self.ring_shown += (wanted - self.ring_shown) * ring_part(if wanted > self.ring_shown { 0.13 } else { 0.05 });
        if active.is_some() {
            self.ring_settled += (1.0 - self.ring_settled) * (1.0 - 9e-4f32.powf(dt));
        } else if self.ring_shown < 0.003 {
            self.ringed = None;
        }
        if let Some((node, at)) = self.ringed.and_then(|node| Some((node, seen.get(node).copied()??))) {
            let gap = 1.1 * (0.5 + 0.35 * (1.0 - self.ring_settled));
            let opacity = self.ring_shown * (0.35 + 0.65 * self.ring_settled);
            let stroke = Stroke::new((0.34 * at.scale).max(1.0), accent.gamma_multiply(opacity));
            painter.circle_stroke(at.pos, (self.radius[node] + gap) * at.scale, stroke);
        }

        // Dots running out along the active note's links.
        let speed = self.speed();
        for dot in &mut self.dots {
            if !dot.on {
                continue;
            }
            dot.along = (dot.along + dot.speed * dt * speed).fract();
            let edge = &self.edges[dot.edge];
            let (a, b) = (self.at[edge.a], self.at[edge.b]);
            let along = if dot.backward { 1.0 - dot.along } else { dot.along };
            let Some(at) = lens.project(curve(a, (a + b) * 0.5 + edge.bend * curl, b, along)) else { continue };
            // Brightest midway, so that they neither pop out of one note nor into the other.
            let radius = (dot.size * 300.0 / at.depth / 2.0).max(0.8);
            painter.circle_filled(at.pos, radius, accent.gamma_multiply((dot.along * PI).sin()));
        }

        // The notes' names, above their discs.
        let names_in = if reveal >= 1.0 { 1.0 } else { smoothstep(0.62, 1.0, reveal) };
        let raise = 1.0 - 9e-4f32.powf(dt);
        let mut names: Vec<Name> = Vec::new();
        // How wide a letter of the names' font is, for each point of its size.
        let letter_width = ui.fonts(|fonts| fonts.glyph_width(&FontId::monospace(MEASURED_AT), 'M')) / MEASURED_AT;
        for &node in &order {
            let stature = (self.radius[node] - 2.2) / 6.5;
            let is_active = active == Some(node);
            // With one note focused, the mouse can still bring another's name forward.
            let pointed = focused.is_some() && !is_active && self.hovered == Some(node);
            self.raised[node] += (f32::from(pointed) - self.raised[node]) * raise;
            let raised = self.raised[node];
            let aside = focused.is_some_and(|focused| !is_active && !is_neighbor(focused, node));

            let above = self.at[node] + Vec3::new(0.0, self.size[node] + 3.0, 0.0);
            let Some(at) = lens.project(above) else { continue };
            let fade = 1.0 - smoothstep(middle - fade_near, middle + FADE_FAR, lens.eye.distance(above)) * LABEL_FADE * fade_cut;
            let mut opacity = (0.3 + 0.5 * stature) * fade * clear[node] * self.shown[node];
            if aside {
                opacity *= 0.05;
            }
            let opacity = names_in * if is_active { 1.0 } else { opacity + (1.0 - opacity) * raised };
            let size = ((2.1 + 2.0 * stature) * at.scale * LABEL_SCALE).min(LARGEST_LABEL);
            if opacity < 0.04 || size < SMALLEST_LABEL {
                continue;
            }
            // Laid out at one of a few sizes, and stretched to its own (see
            // `laid_out_at` and `stretched`). A letter's width says how
            // large the glyphs really came out, and the widths of all the
            // letters how wide the name would be were they not set at whole
            // pixels.
            let font = FontId::monospace(laid_out_at(size, ui.ctx().pixels_per_point()));
            let title = &self.titles[node];
            let (letter, letters) = ui.fonts(|fonts| (fonts.glyph_width(&font, 'M'), title.chars().map(|c| fonts.glyph_width(&font, c)).sum::<f32>()));
            let text = painter.layout_no_wrap(title.clone(), font, Color32::PLACEHOLDER);
            let taller = size * letter_width / letter.max(0.01);
            let text = stretched(text.clone(), taller * letters / text.size().x.max(0.01), taller);
            let corner = at.pos - vec2(text.size().x / 2.0, text.size().y);
            // The active note's name comes before any other, then the one the
            // mouse is on, then the clearest.
            let rank = if is_active { 3.0 } else { opacity + raised };
            names.push(Name { node, corner, text, color: if is_active { accent } else { ink }, opacity, rank });
        }
        // The site writes every name, one over another. Here a name that
        // would cross a more important one steps back until there is room,
        // which is what keeps a tight group of notes readable.
        names.sort_by(|a, b| b.rank.total_cmp(&a.rank));
        let mut written: Vec<Rect> = Vec::new();
        let give_way = 1.0 - (-dt / 0.12).exp();
        for name in names {
            let space = Rect::from_min_size(name.corner, name.text.size()).expand2(vec2(3.0, 0.0));
            let clear = !written.iter().any(|other| other.intersects(space));
            if clear {
                written.push(space);
            }
            let named = &mut self.named[name.node];
            *named += (f32::from(clear) - *named) * give_way;
            let shown = name.opacity * *named;
            if shown >= 0.04 {
                // A name is the colour of the discs, and a large disc behind
                // it, or one in front of its own, swallowed it. So it is cut
                // out of what is under it by a rim of the page's colour, the
                // name written in that colour a little way off on every side.
                // The site has none. A name with nothing under it needs none
                // either, and most have nothing: a rim is the name drawn
                // eight times over, which is most of what a frame sends the
                // GPU, so it is drawn only where a disc is.
                let rim = (name.text.size().y * 0.08).clamp(0.8, 2.4);
                let room = Rect::from_min_size(name.corner, name.text.size()).expand(rim);
                if discs.iter().any(|&(center, radius)| room.distance_sq_to_pos(center) < radius * radius) {
                    for step in 0..8 {
                        let angle = step as f32 * TAU / 8.0;
                        let off = vec2(angle.cos(), angle.sin()) * rim;
                        painter.galley(name.corner + off, name.text.clone(), paper.gamma_multiply(shown));
                    }
                }
                painter.galley(name.corner, name.text, name.color.gamma_multiply(shown));
            }
        }

        // What can be done here, in a line along the bottom.
        let caption = match focused {
            Some(node) => format!("{}  ·  click it again or press Enter to open  ·  Esc to step back", self.titles[node]),
            None if self.rest.is_empty() => "There are no notes in this vault yet".to_owned(),
            None if self.found.is_some() => "Only the notes found are shown  ·  Enter goes to each in turn  ·  Esc to see them all".to_owned(),
            None => "Drag to turn  ·  scroll to zoom  ·  click a note, or Ctrl+F to find one  ·  Esc to close".to_owned(),
        };
        let caption = painter.layout_no_wrap(caption, egui::TextStyle::Small.resolve(ui.style()), ink.gamma_multiply(0.55));
        painter.galley(pos2(view.center().x - caption.size().x / 2.0, view.bottom() - caption.size().y - 8.0), caption, ink);
        outcome
    }
}

/// Where along the curve from `a` to `b` an edge is drawn, as `count` stops
/// evenly spaced between the rim of the disc at one end and the rim of the
/// disc at the other. Empty if the two discs leave nothing between them.
fn between_rims(a: Vec3, b: Vec3, bend: Vec3, radius_a: f32, radius_b: f32, count: usize) -> Vec<f32> {
    const PIECES: usize = 24;
    let middle = (a + b) * 0.5 + bend;
    let mut length = [0.0; PIECES + 1];
    for piece in 0..PIECES {
        let (from, to) = (curve(a, middle, b, piece as f32 / PIECES as f32), curve(a, middle, b, (piece + 1) as f32 / PIECES as f32));
        length[piece + 1] = length[piece] + from.distance(to);
    }
    let end = length[PIECES] - radius_b;
    if end <= radius_a {
        return Vec::new();
    }
    // The stop that is `along` the curve, measured in distance.
    let stop_at = |along: f32| {
        let piece = (0..PIECES).find(|&piece| length[piece + 1] >= along).unwrap_or(PIECES - 1);
        let within = (along - length[piece]) / (length[piece + 1] - length[piece]).max(1e-6);
        (piece as f32 + within) / PIECES as f32
    };
    (0..count).map(|stop| stop_at(radius_a + (end - radius_a) * stop as f32 / (count - 1).max(1) as f32)).collect()
}

impl App {
    /// Show the graph, or put it away again.
    pub(crate) fn toggle_graph(&mut self) {
        self.show_graph = !self.show_graph;
        if self.show_graph {
            // What cannot be seen is not searched.
            self.find = None;
        }
    }

    /// The note the graph has in focus, by its place in `notes`, while the
    /// graph is what is showing. A first click on a note in the graph looks
    /// at it without opening it, and this is how the side panel knows which
    /// note is being looked at.
    pub(crate) fn graph_focus(&self) -> Option<usize> {
        let focused = self.graph.as_ref().filter(|_| self.show_graph)?.focused_path()?;
        // By its file, not its number: the graph may be a change of the notes behind.
        self.notes.iter().position(|note| note.path == *focused)
    }

    /// The graph, where the note otherwise is.
    pub(crate) fn graph_view(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let theme = self.theme(ctx);
        // There is no editor to take these while the graph is up.
        self.pending_edit = None;
        self.jump = None;
        let page = egui::Frame::central_panel(&ctx.style()).fill(theme.page);
        egui::CentralPanel::default().frame(page).show(ctx, |ui| {
            let open = self.open.clone();
            self.header(ui, open.as_deref(), actions);
            ui.separator();
            // A note the bar has found, or one a link in the side panel names.
            let looked = self.graph_look.take().and_then(|path| self.notes.iter().position(|note| note.path == path));
            let sought = self.graph_find_bar(ui).or(looked);
            let now = ui.input(|input| input.time);
            let mut graph = match self.graph.take() {
                Some(graph) if graph.version == self.version => graph,
                before => GraphView::build(&self.notes, self.version, now, before),
            };
            let view = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(view, Sense::click_and_drag());
            // While the bar is up, the graph is of what it has found.
            graph.search(self.graph_find.as_ref().map(crate::find::GraphFind::found), view.size(), now);
            if let Some(note) = sought {
                // The graph is of the notes as they are now, so a note's
                // place among them is its place in the graph. Fog is no
                // obstacle here as it is to the mouse: the camera goes round.
                graph.focus(Some(note), view.size(), now);
            }
            // The keyboard is the graph's unless a dialog or a text field has it.
            let keys = self.picker.is_none() && self.confirm.is_none() && ctx.memory(|memory| memory.focused().is_none());
            match graph.frame(ui, view, &response, &theme, keys) {
                Some(Outcome::Open(path)) => actions.extend([Action::OpenAt(path), Action::FocusEditor]),
                Some(Outcome::Close) => self.show_graph = false,
                Some(Outcome::StopSearching) => self.graph_find = None,
                None => {}
            }
            let window_in_front = ctx.input(|input| input.focused);
            let pace = graph.frames_per_second(now, window_in_front);
            self.graph = Some(graph);
            // Drawn no more often than it needs, and not at all at rest.
            if let Some(pace) = pace {
                ctx.request_repaint_after(std::time::Duration::from_secs_f32(1.0 / pace));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0))
    }

    #[test]
    fn names_are_laid_out_at_few_sizes_and_never_far_from_their_own() {
        for pixels_per_point in [1.0, 1.25, 1.56, 2.0] {
            let mut laid_out = Vec::new();
            let mut size = SMALLEST_LABEL;
            while size <= LARGEST_LABEL {
                let at = laid_out_at(size, pixels_per_point);
                assert!((size / at - 1.0).abs() < 0.15, "a name of {size} is laid out at {at}");
                let pixels = at * pixels_per_point;
                assert!((pixels - pixels.round()).abs() < 1e-3, "{at} is {pixels} pixels: glyphs are made at whole ones");
                if laid_out.last() != Some(&at) {
                    laid_out.push(at);
                }
                size += 0.01;
            }
            assert!(laid_out.len() <= 11, "{laid_out:?}");
            assert!(laid_out.windows(2).all(|pair| pair[0] < pair[1]), "and the same size every time: {laid_out:?}");
        }
    }

    #[test]
    fn the_camera_looks_at_its_target_from_where_it_was_put() {
        let pose = Pose::looking_from(home_direction(), 500.0, Vec3::new(10.0, 20.0, 30.0));
        let eye = pose.eye();
        assert!((eye.distance(pose.target) - 500.0).abs() < 0.01);
        let direction = (eye - pose.target).normalized_or(Vec3::ZERO);
        assert!(direction.distance(home_direction()) < 1e-4);

        let lens = Lens::new(&pose, view());
        let centre = lens.project(pose.target).unwrap();
        assert!(centre.pos.distance(view().center()) < 0.01, "what it looks at is in the middle of the view");
        assert!((centre.depth - 500.0).abs() < 0.01);
        assert!(lens.project(eye + (eye - pose.target)).is_none(), "what is behind it is not seen");
    }

    #[test]
    fn up_is_up_and_nearer_is_bigger() {
        let pose = Pose::looking_from(Vec3::new(0.0, 0.0, 1.0), 400.0, Vec3::ZERO);
        let lens = Lens::new(&pose, view());
        let at = |x: f32, y: f32, z: f32| lens.project(Vec3::new(x, y, z)).unwrap();
        assert!(at(0.0, 50.0, 0.0).pos.y < view().center().y, "up in the graph is up on the screen");
        assert!(at(50.0, 0.0, 0.0).pos.x > view().center().x, "and right is right");
        assert!(at(0.0, 0.0, 100.0).scale > at(0.0, 0.0, -100.0).scale);
        // At the top of the field of view, 25 degrees up, a point is on the view's top edge.
        let top = at(0.0, 400.0 * HALF_FOV.tan(), 0.0);
        assert!((top.pos.y - view().top()).abs() < 0.01);
    }

    #[test]
    fn the_ball_is_framed_by_its_narrower_side() {
        let wide = fit_distance(vec2(1600.0, 800.0), 230.0);
        assert!((wide - 230.0 / HALF_FOV.sin() * 0.85).abs() < 0.01, "a wide view fits it top to bottom");
        let square = fit_distance(vec2(800.0, 801.0), 230.0);
        let tall = fit_distance(vec2(400.0, 800.0), 230.0);
        assert!(tall > square, "a narrow view has to stand further back");
        assert!(fit_distance(vec2(10.0, 800.0), 230.0) <= FARTHEST * 0.52 + 0.01);
        assert!(fit_distance(vec2(800.0, 0.0), 230.0).is_finite());
    }

    #[test]
    fn an_edge_stops_at_the_rims_of_its_discs() {
        let (a, b) = (Vec3::new(-50.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0));
        let stops = between_rims(a, b, Vec3::ZERO, 10.0, 20.0, 16);
        assert_eq!(stops.len(), 16);
        assert!((stops[0] - 0.1).abs() < 1e-3 && (stops[15] - 0.8).abs() < 1e-3, "{stops:?}");
        assert!(stops.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(between_rims(a, b, Vec3::ZERO, 60.0, 50.0, 16).is_empty(), "discs that meet leave no edge to draw");
        assert_eq!(between_rims(a, b, Vec3::ZERO, 10.0, 20.0, 2).len(), 2);
    }

    /// Two notes with a link between them, drawn till they have arrived and
    /// then set one behind the other as the camera sees them, a little to
    /// one side: the far one's disc shows from behind the near one's. With
    /// `still`, they are kept from drifting and the camera from turning, so
    /// that what moves is only what the mouse makes move. With the graph
    /// come the window it is drawn in, a clock, and the view.
    fn one_behind_another(still: bool) -> (GraphView, egui::Context, f64, Rect, [usize; 2]) {
        let note = |title: &str, text: &str| Note { title: title.to_owned(), path: PathBuf::from(format!("{title}.md")), content: text.to_owned() };
        let notes = [note("Near", "[[Far]]"), note("Far", "[[Near]]"), note("Other", "")];
        let mut graph = GraphView::build(&notes, 0, 0.0, None);
        let (ctx, mut clock) = (egui::Context::default(), 0.0);
        let (_, view) = watch(&mut graph, &ctx, &mut clock, None, 480);
        if still {
            for motion in &mut graph.motion {
                motion.reach = [0.0; 3];
            }
            graph.touched = f64::MAX;
            graph.turning = Vec2::ZERO;
        }
        let lens = Lens::new(&graph.camera, view);
        let (near, far) = (0, 1);
        graph.rest[near] = Vec3::ZERO;
        graph.rest[far] = lens.forward * 40.0 + lens.right * (1.5 * graph.radius[near]);
        graph.rest[2] = lens.right * -200.0;
        watch(&mut graph, &ctx, &mut clock, None, 120);
        (graph, ctx, clock, view, [near, far])
    }

    /// Draw `frames` frames of the graph with the mouse at `pointer`, and
    /// say which note it was on in each, and where the graph was drawn.
    fn watch(graph: &mut GraphView, ctx: &egui::Context, clock: &mut f64, pointer: Option<Pos2>, frames: usize) -> (Vec<Option<usize>>, Rect) {
        let theme = Theme::new(true, false);
        let (mut on, mut drawn) = (Vec::new(), Rect::NOTHING);
        for _ in 0..frames {
            *clock += 1.0 / 60.0;
            let events = pointer.map(egui::Event::PointerMoved).into_iter().collect();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));
            let input = egui::RawInput { screen_rect: Some(screen), time: Some(*clock), events, ..Default::default() };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let view = ui.available_rect_before_wrap();
                    let response = ui.allocate_rect(view, Sense::click_and_drag());
                    graph.frame(ui, view, &response, &theme, false);
                    drawn = view;
                });
            });
            on.push(graph.hovered);
        }
        (on, drawn)
    }

    /// Where a note is drawn, and how big its disc is there.
    fn disc(graph: &GraphView, view: Rect, node: usize) -> (Pos2, f32) {
        let seen = Lens::new(&graph.camera, view).project(graph.at[node]).expect("the note is in front of the camera");
        (seen.pos, graph.radius[node] * seen.scale)
    }

    #[test]
    fn the_mouse_stays_on_the_note_it_is_on_with_another_behind_it() {
        // With nothing moving but what the mouse moves, and then with the
        // notes adrift and the camera turning as they do.
        for still in [true, false] {
            let (mut graph, ctx, mut clock, view, [near, far]) = one_behind_another(still);
            let ((near_at, near_size), (far_at, far_size)) = (disc(&graph, view, near), disc(&graph, view, far));
            assert!(near_at.distance(far_at) < near_size + far_size, "the two discs overlap: {near_at:?} {near_size}, {far_at:?} {far_size}");
            // Between the two of them, where both are in reach, for five seconds.
            let between = near_at + (far_at - near_at) * 0.5;
            let (on, _) = watch(&mut graph, &ctx, &mut clock, Some(between), 300);
            let changes = on.windows(2).filter(|pair| pair[0] != pair[1]).count();
            assert!(on[0].is_some(), "it is on a note");
            // What drifts out from under the pointer is let go of, the once.
            assert!(changes <= usize::from(!still), "and stays on it: {changes} changes, {:?}", &on[..on.len().min(60)]);
        }
    }

    #[test]
    fn of_two_discs_the_mouse_is_on_the_one_in_front_and_that_note_stands_still() {
        let (mut graph, ctx, mut clock, view, [near, far]) = one_behind_another(true);
        let ((near_at, near_size), (far_at, _)) = (disc(&graph, view, near), disc(&graph, view, far));
        // On the near disc, at the side the far one is: nearer the middle of the far one than its own.
        let towards_far = (far_at - near_at).normalized();
        let on_near = near_at + towards_far * (0.9 * near_size);
        assert!(on_near.distance(far_at) < on_near.distance(near_at), "{near_at:?} {far_at:?} {near_size}");
        let (on, _) = watch(&mut graph, &ctx, &mut clock, Some(on_near), 90);
        assert!(on.iter().all(|on| *on == Some(near)), "the disc in front is the one pointed at: {on:?}");
        assert!(graph.lean[far].length() > 0.5, "and the note behind has leant towards it");

        // Off the near disc, though still in the room round it, and on the
        // far one where it shows from behind: that is the far note's.
        let ((near_at, near_size), (far_at, far_size)) = (disc(&graph, view, near), disc(&graph, view, far));
        let on_far = near_at + (far_at - near_at).normalized() * (1.15 * near_size);
        assert!(on_far.distance(far_at) < far_size, "{near_at:?} {near_size}, {far_at:?} {far_size}");
        watch(&mut graph, &ctx, &mut clock, Some(on_far), 2);
        assert_eq!(graph.hovered, Some(far));
        // It was leaning, and under the mouse it stays where it is and does
        // not go back to its place. The near note now leans this way, over
        // the pointer and in front: a pointer held still stays with its note.
        let (held, leant) = (graph.at[far], graph.lean[far]);
        assert!(leant.length() > 0.5);
        let (on, _) = watch(&mut graph, &ctx, &mut clock, Some(on_far), 120);
        assert!(on.iter().all(|on| *on == Some(far)), "{on:?}");
        assert!(graph.at[far].distance(held) < 0.05 && graph.lean[far].distance(leant) < 0.05, "{held:?} to {:?}", graph.at[far]);
        let (near_at, near_size) = disc(&graph, view, near);
        assert!(on_far.distance(near_at) < near_size, "the near disc has come over the pointer: {near_at:?} {near_size}");
        // The least move of the hand, and it is on what is now on top.
        watch(&mut graph, &ctx, &mut clock, Some(on_far + vec2(0.5, 0.0)), 2);
        assert_eq!(graph.hovered, Some(near));

        // Back onto the near disc, still within reach of the far one: the one in front has it.
        let ((near_at, near_size), (far_at, far_size)) = (disc(&graph, view, near), disc(&graph, view, far));
        let back = near_at + (far_at - near_at).normalized() * (0.5 * near_size);
        assert!(back.distance(far_at) < REACH * far_size, "{near_at:?} {near_size}, {far_at:?} {far_size}");
        let (on, _) = watch(&mut graph, &ctx, &mut clock, Some(back), 120);
        assert!(on.iter().all(|on| *on == Some(near)), "{on:?}");
        // And with the mouse gone, everything goes back to where it rests.
        watch(&mut graph, &ctx, &mut clock, Some(view.left_top() + vec2(5.0, 5.0)), 240);
        assert_eq!(graph.hovered, None);
        assert!(graph.lean[near].length() < 0.05 && graph.lean[far].length() < 0.05);
    }

    #[test]
    fn things_arrive_fast_and_settle() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.3) > 0.8);
        let mut value = 1.0;
        attend(&mut value, 2.0, 0.1);
        let risen = value - 1.0;
        let mut value = 1.0;
        attend(&mut value, 0.0, 0.1);
        assert!(risen > 0.0 && risen < 1.0 - value && value > 0.0, "dimming is the quicker of the two, and both are gradual");
        assert!((0.0..1.0).contains(&scatter(3.0)) && scatter(3.0) == scatter(3.0) && scatter(3.0) != scatter(4.0));
    }
}
