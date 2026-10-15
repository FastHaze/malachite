//! The vault as a graph: every note a node, every `[[wikilink]]` between two
//! notes an edge, laid out in space for the graph view to draw.
//!
//! The view is modelled on the graph of aicodingdictionary.com, and so are
//! the numbers here: a node is sized by its links, and the layout is a force
//! simulation with that site's forces (it repels every pair, keeps discs
//! from touching, pulls linked notes together and everything gently to the
//! middle), which settles into a ball of roughly the size that site's camera
//! and fades are tuned for.
//!
//! The site sizes a node by the links to it alone. Here a link counts for
//! the notes at both its ends: a note that lists twenty others, an index of
//! them, is as much the middle of something as a note twenty others point
//! to, and drawn at the smallest size with twenty lines to it, it looked
//! like a mistake.

use std::collections::{HashMap, HashSet};
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};

use crate::{aliases, wikilinks, Note};

/// A point or a direction in the graph's space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(self.y * other.z - self.z * other.y, self.z * other.x - self.x * other.z, self.x * other.y - self.y * other.x)
    }

    /// The same direction at length 1, or `fallback` for a vector too short to have one.
    pub fn normalized_or(self, fallback: Self) -> Self {
        let length = self.length();
        if length > 1e-4 {
            self * (1.0 / length)
        } else {
            fallback
        }
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, by: f32) -> Self {
        Self::new(self.x * by, self.y * by, self.z * by)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

/// The smallest a node is drawn, and how much the one with the most links adds to that.
const RADIUS: f32 = 2.2;
const RADIUS_RANGE: f32 = 6.5;

/// Room kept around every disc, on top of its radius.
const PADDING: f32 = 18.0;
/// How far apart, edge to edge, a link would have two notes be.
const LINK_GAP: f32 = 40.0;
const LINK_STRENGTH: f32 = 0.08;
/// How hard every node pushes every other away, and is drawn to the middle.
const CHARGE: f32 = 55.0;
const CENTRE: f32 = 0.06;

// The site's forces are tuned for its own graph, in which links are spread
// evenly. A vault is not like that: a few groups of notes all link to one
// another, and most notes link to nothing. Left as they are, the forces pull
// such a group into a knot and give the room to the loose notes. These make
// up for it.
/// How much further apart a link lets two notes be for each of the links the
/// busier of them has (by the square root, so hubs do not fly off).
const LINK_GAP_PER_LINK: f32 = 12.0;
/// How much harder two linked notes push each other away for each link they
/// have (again by the square root). Only between notes that both have links:
/// it is to open a group out, not to drive it away from everything else.
const CHARGE_PER_LINK: f32 = 0.4;
/// How hard a note with no links pushes and is pushed, next to one that has
/// some. Loose notes have nothing to show but themselves and can stand
/// closer, which leaves the room for the groups.
const LOOSE_CHARGE: f32 = 0.45;
/// The least room, edge to edge, left between any two discs in the end.
const CLEARANCE: f32 = 14.0;

/// How long the simulation runs, and the most pairs of nodes it will weigh
/// against each other in all: a large vault gets fewer rounds rather than a
/// long wait.
const TICKS: usize = 180;
const MIN_TICKS: usize = 12;
const PAIR_BUDGET: usize = 20_000_000;

/// How much the ball is opened out once it has settled, and the widest it
/// is left. The forces pack it tighter than the site's own ball, which the
/// view's camera and fades are tuned for; this brings it to that scale.
const OPENING: f32 = 1.6;
const MAX_EXTENT: f32 = 200.0;

pub struct Node {
    /// The notes it links to or is linked from, each once. How many there
    /// are is how many lines the note has, and what it is sized by.
    pub neighbors: Vec<usize>,
}

/// Nodes are in the order of the notes the graph was built from: node `i` is note `i`.
pub struct Graph {
    pub nodes: Vec<Node>,
    /// Each pair of linked notes once, whichever way the link goes, lower index first.
    pub edges: Vec<(usize, usize)>,
    /// How many notes the note with the most links is linked with.
    most_links: usize,
}

impl Graph {
    /// Read the links out of `notes`. A link counts when it names a note that
    /// exists (by title, capitals aside, the first such note when several
    /// folders have one, or failing a title by alias) other than the note it
    /// is in.
    pub fn build(notes: &[Note]) -> Self {
        let mut by_title: HashMap<String, usize> = HashMap::new();
        for (index, note) in notes.iter().enumerate() {
            by_title.entry(note.title.to_lowercase()).or_insert(index);
        }
        // After all the titles, so that an alias never takes a title's place.
        for (index, note) in notes.iter().enumerate() {
            for alias in aliases(&note.content) {
                by_title.entry(alias.to_lowercase()).or_insert(index);
            }
        }
        let mut links: HashSet<(usize, usize)> = HashSet::new();
        for (from, note) in notes.iter().enumerate() {
            for target in wikilinks(&note.content) {
                if let Some(&to) = by_title.get(&target.to_lowercase()).filter(|&&to| to != from) {
                    links.insert((from, to));
                }
            }
        }

        let mut nodes: Vec<Node> = notes.iter().map(|_| Node { neighbors: Vec::new() }).collect();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for &(from, to) in &links {
            // A link each way is still one line between the two.
            if from < to || !links.contains(&(to, from)) {
                edges.push((from.min(to), from.max(to)));
                nodes[from].neighbors.push(to);
                nodes[to].neighbors.push(from);
            }
        }
        edges.sort_unstable();
        for node in &mut nodes {
            node.neighbors.sort_unstable();
        }
        let most_links = nodes.iter().map(|node| node.neighbors.len()).max().unwrap_or(0);
        Self { nodes, edges, most_links }
    }

    /// How big node `index` is drawn: larger the more notes it is linked
    /// with, whichever way the links go, on a scale that flattens out, so
    /// one hub does not dwarf everything else.
    pub fn radius(&self, index: usize) -> f32 {
        let most = (self.most_links.max(1) as f32).ln_1p();
        RADIUS + (self.nodes[index].neighbors.len() as f32).ln_1p() / most * RADIUS_RANGE
    }

    /// Where each node rests. The same notes always give the same picture.
    pub fn layout(&self) -> Vec<Vec3> {
        let count = self.nodes.len();
        let radius: Vec<f32> = (0..count).map(|index| self.radius(index)).collect();
        // Begin spread through a ball, on a spiral that never puts two nodes together.
        let spread = 30.0 * (count as f32).cbrt();
        let mut at: Vec<Vec3> = (0..count)
            .map(|index| {
                let along = (index as f32 + 0.5) / count as f32;
                let (height, turn) = (1.0 - 2.0 * along, 2.399_963 * index as f32);
                let ring = (1.0 - height * height).max(0.0).sqrt();
                Vec3::new(ring * turn.cos(), height, ring * turn.sin()) * (spread * along.cbrt())
            })
            .collect();
        let mut speed = vec![Vec3::ZERO; count];
        let links: Vec<f32> = self.nodes.iter().map(|node| node.neighbors.len() as f32).collect();
        let charge: Vec<f32> = links.iter().map(|&links| 1.0 + CHARGE_PER_LINK * links.sqrt()).collect();

        let pairs = (count * count / 2).max(1);
        let ticks = (PAIR_BUDGET / pairs).clamp(MIN_TICKS, TICKS);
        // The simulation cools as it goes, as d3-force's does, whatever its length.
        let cooling = 1.0 - 0.001_f32.powf(1.0 / ticks as f32);
        let mut heat = 1.0;
        for _ in 0..ticks {
            heat -= heat * cooling;
            for &(a, b) in &self.edges {
                let between = at[b] - at[a];
                let length = between.length().max(1.0);
                // Each of a note's many links pulls less, or together they
                // would outweigh everything that keeps its neighbours apart.
                let (busier, quieter) = (links[a].max(links[b]), links[a].min(links[b]));
                let rest = radius[a] + radius[b] + LINK_GAP + LINK_GAP_PER_LINK * busier.sqrt();
                let pull = between * ((length - rest) / length * LINK_STRENGTH / quieter.max(1.0));
                at[a] += pull;
                at[b] -= pull;
            }
            for a in 0..count {
                for b in a + 1..count {
                    let between = at[b] - at[a];
                    let apart = between.dot(between).max(1.0);
                    let linked = links[a] > 0.0 && links[b] > 0.0;
                    let push = between * (CHARGE * if linked { charge[a] * charge[b] } else { LOOSE_CHARGE } * heat / apart);
                    speed[a] -= push;
                    speed[b] += push;
                    let room = radius[a] + radius[b] + 2.0 * PADDING;
                    if apart < room * room {
                        let apart = apart.sqrt();
                        let shove = between * ((room - apart) / apart * 0.5);
                        speed[a] -= shove;
                        speed[b] += shove;
                    }
                }
                speed[a] -= at[a] * (CENTRE * heat);
            }
            for (at, speed) in at.iter_mut().zip(&mut speed) {
                *speed = *speed * 0.6;
                *at += *speed;
            }
        }

        // Centred on the middle, where the camera looks, and at the scale it is tuned for.
        let middle = at.iter().fold(Vec3::ZERO, |sum, &at| sum + at) * (1.0 / count.max(1) as f32);
        let extent = at.iter().map(|at| at.distance(middle)).fold(0.0, f32::max);
        let scale = if extent > 0.0 { (extent * OPENING).min(MAX_EXTENT) / extent } else { 1.0 };
        for at in &mut at {
            *at = (*at - middle) * scale;
        }
        // Whatever the forces left too close is eased apart, so that no disc
        // covers another and there is air between any two.
        for _ in 0..80 {
            let mut touching = false;
            for a in 0..count {
                for b in a + 1..count {
                    let between = at[b] - at[a];
                    let (apart, room) = (between.length(), radius[a] + radius[b] + CLEARANCE);
                    if apart < room {
                        touching = true;
                        // Two nodes in one spot have no direction to part in: give them one.
                        let way = between.normalized_or(Vec3::new(1.0, 0.0, 0.0));
                        let shove = way * ((room - apart) * 0.5 + 0.01);
                        at[a] -= shove;
                        at[b] += shove;
                    }
                }
            }
            if !touching {
                break;
            }
        }
        at
    }

    /// Where the notes a search has found are gathered to, in the order of
    /// `found`, about the middle of the graph: out of the places they rest
    /// at (`rest`, from `layout`), to be seen together, close but clear of
    /// each other, those linked with each other nearer. One note found is
    /// put in the middle.
    ///
    /// This is the site's: a small simulation of the notes found alone, as
    /// d3-force runs it, started from where they rest, with the site's own
    /// forces and none of `layout`'s changes to them. There are few notes
    /// to it, and nothing for the changes to make up for.
    pub fn gather(&self, rest: &[Vec3], found: &[usize]) -> Vec<Vec3> {
        let count = found.len();
        if count < 2 {
            return vec![Vec3::ZERO; count];
        }
        let middle = found.iter().fold(Vec3::ZERO, |sum, &node| sum + rest[node]) * (1.0 / count as f32);
        let mut at: Vec<Vec3> = found.iter().map(|&node| rest[node] - middle).collect();
        let mut speed = vec![Vec3::ZERO; count];
        let radius: Vec<f32> = found.iter().map(|&node| self.radius(node)).collect();
        let place: HashMap<usize, usize> = found.iter().enumerate().map(|(index, &node)| (node, index)).collect();
        let links: Vec<(usize, usize)> = self.edges.iter().filter_map(|(a, b)| Some((*place.get(a)?, *place.get(b)?))).collect();
        // d3-force's own cooling, from 1 down to a thousandth over 300 ticks.
        let cooling = 1.0 - 0.001_f32.powf(1.0 / 300.0);
        let mut heat = 1.0;
        // Two notes in one spot have no direction to part in: give them one.
        let apart = |between: Vec3, a: usize, b: usize| {
            if between == Vec3::ZERO {
                Vec3::new(1e-3 * (a as f32 - b as f32), 1e-3, 0.0)
            } else {
                between
            }
        };
        for _ in 0..TICKS {
            heat -= heat * cooling;
            // Every pair repels.
            for a in 0..count {
                for b in a + 1..count {
                    let between = apart(at[b] - at[a], a, b);
                    let push = between * (-CHARGE * heat / between.dot(between).max(1.0));
                    speed[a] += push;
                    speed[b] -= push;
                }
            }
            // No disc comes within a little way of another, each judged by
            // where it is about to be.
            for _ in 0..4 {
                for a in 0..count {
                    for b in a + 1..count {
                        let between = apart((at[a] + speed[a]) - (at[b] + speed[b]), a, b);
                        let room = radius[a] + radius[b] + 2.0 * PADDING;
                        let length = between.length();
                        if length < room {
                            let shove = between * ((room - length) / length);
                            // The smaller of the two gives way the more.
                            let share = radius[b] * radius[b] / (radius[a] * radius[a] + radius[b] * radius[b]);
                            speed[a] += shove * share;
                            speed[b] -= shove * (1.0 - share);
                        }
                    }
                }
            }
            for index in 0..count {
                speed[index] -= at[index] * (CENTRE * heat);
            }
            // A link holds its two notes at a little more than their discs apart.
            for &(a, b) in &links {
                let between = at[b] - at[a];
                let length = between.length().max(1.0);
                let pull = between * ((length - (radius[a] + radius[b] + LINK_GAP)) / length * LINK_STRENGTH);
                at[a] += pull;
                at[b] -= pull;
            }
            for (at, speed) in at.iter_mut().zip(&mut speed) {
                *speed = *speed * 0.6;
                *at += *speed;
            }
        }
        let middle = at.iter().fold(Vec3::ZERO, |sum, &at| sum + at) * (1.0 / count as f32);
        at.iter().map(|&at| at - middle).collect()
    }
}

/// The point an edge between `a` and `b` bends towards: out from the middle
/// of the ball, further the longer the edge, so that long links arc around
/// the ball rather than cutting through it.
pub fn control(a: Vec3, b: Vec3) -> Vec3 {
    let middle = (a + b) * 0.5;
    let across = b - a;
    // An edge through the very middle has no "out": bend it sideways instead.
    let sideways = across.cross(Vec3::new(0.0, 1.0, 0.0)).normalized_or(Vec3::new(1.0, 0.0, 0.0));
    middle + middle.normalized_or(sideways) * (0.22 * across.length())
}

/// A point on the curve from `a` to `b` that bends towards `control`, `t` of the way along.
pub fn curve(a: Vec3, control: Vec3, b: Vec3, t: f32) -> Vec3 {
    let rest = 1.0 - t;
    a * (rest * rest) + control * (2.0 * rest * t) + b * (t * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn notes(list: &[(&str, &str)]) -> Vec<Note> {
        list.iter().map(|(path, content)| note(path, content)).collect()
    }

    fn note(path: &str, content: &str) -> Note {
        let path = PathBuf::from(path);
        Note { title: path.file_stem().unwrap().to_string_lossy().into_owned(), path, content: content.to_string() }
    }

    #[test]
    fn links_between_notes_become_edges() {
        let graph = Graph::build(&notes(&[("A.md", "[[B]] and [[c|see]] and [[B#Part]]"), ("B.md", "[[A]]"), ("Sub/C.md", "")]));
        assert_eq!(graph.edges, [(0, 1), (0, 2)], "one edge for a link each way, and for a link made twice");
        assert_eq!(graph.nodes[0].neighbors, [1, 2]);
        assert_eq!(graph.nodes[2].neighbors, [0]);
        let lines: Vec<usize> = graph.nodes.iter().map(|node| node.neighbors.len()).collect();
        assert_eq!(lines, [2, 1, 1]);
    }

    #[test]
    fn a_link_by_alias_is_an_edge_unless_a_note_has_that_title() {
        let aliased = "---\naliases: [Bee, C]\n---\n";
        let graph = Graph::build(&notes(&[("A.md", "[[bee]] [[C]]"), ("B.md", aliased), ("C.md", "")]));
        assert_eq!(graph.edges, [(0, 1), (0, 2)]);
    }

    #[test]
    fn links_to_nowhere_to_itself_and_in_code_do_not_count() {
        let graph = Graph::build(&notes(&[("A.md", "[[A]] [[Missing]] `[[B]]`\n```\n[[B]]\n```"), ("B.md", "")]));
        assert!(graph.edges.is_empty());
        assert!(graph.nodes[1].neighbors.is_empty());
        assert!(Graph::build(&[]).layout().is_empty());
    }

    #[test]
    fn a_note_is_bigger_the_more_notes_it_is_linked_with_either_way() {
        // An index of three notes, one of which links back to it and on to a fourth; and a note apart.
        let list = [("Index.md", "[[A]] [[B]] [[C]]"), ("A.md", ""), ("B.md", "[[Index]] [[D]]"), ("C.md", ""), ("D.md", ""), ("Apart.md", "")];
        let graph = Graph::build(&notes(&list));
        let [index, a, b, _, d, apart] = [0, 1, 2, 3, 4, 5].map(|node| graph.radius(node));
        assert_eq!(apart, RADIUS, "a note with no links is the smallest");
        assert_eq!(index, RADIUS + RADIUS_RANGE, "the one with the most is the biggest, though it is the one doing the linking");
        assert_eq!(a, d, "a link counts the same for the note it is in and the note it is to");
        assert!(a > apart && b > a && b < index);
        assert_eq!(graph.nodes[0].neighbors.len(), 3, "and a link each way is one link");
        let alone = Graph::build(&notes(&[("A.md", "")]));
        assert_eq!(alone.radius(0), RADIUS, "with no links at all there is nothing to divide by");
    }

    /// A vault of `count` notes in which each links to a few of the others.
    fn vault(count: usize) -> Vec<Note> {
        (0..count)
            .map(|index| {
                let links: String = [1, 7, 31].iter().map(|step| format!("[[N{}]] ", (index * 3 + step) % count)).collect();
                note(&format!("N{index}.md"), &links)
            })
            .collect()
    }

    #[test]
    fn the_layout_is_the_same_every_time_and_keeps_discs_apart() {
        let graph = Graph::build(&vault(60));
        let layout = graph.layout();
        assert_eq!(layout, graph.layout());
        assert!(layout.iter().all(|at| at.x.is_finite() && at.y.is_finite() && at.z.is_finite()));
        for a in 0..layout.len() {
            for b in a + 1..layout.len() {
                assert!(layout[a].distance(layout[b]) >= graph.radius(a) + graph.radius(b), "{a} and {b} overlap");
            }
        }
        let extent = layout.iter().map(|at| at.length()).fold(0.0, f32::max);
        assert!((40.0..=MAX_EXTENT + 1.0).contains(&extent), "a ball of a size the view is made for, not {extent}");
    }

    #[test]
    fn a_group_of_notes_that_all_link_to_each_other_is_not_a_knot() {
        // Twenty notes each linking to all the others, among a hundred that link to nothing.
        let group: String = (0..20).map(|index| format!("[[G{index}]] ")).collect();
        let mut list: Vec<Note> = (0..20).map(|index| note(&format!("G{index}.md"), &group)).collect();
        list.extend((0..100).map(|index| note(&format!("Loose{index}.md"), "")));
        let graph = Graph::build(&list);
        let layout = graph.layout();
        let nearest = |a: usize| (0..layout.len()).filter(|&b| b != a).map(|b| layout[a].distance(layout[b])).fold(f32::MAX, f32::min);
        let in_group = (0..20).map(nearest).sum::<f32>() / 20.0;
        let loose = (20..120).map(nearest).sum::<f32>() / 100.0;
        assert!(in_group > 0.7 * loose, "the group's notes are {in_group} from their nearest, loose ones {loose}");
        let width = (0..20).map(|a| layout[a].distance(layout[0])).fold(0.0, f32::max);
        assert!(width > 55.0, "the group is only {width} across");
    }

    #[test]
    fn linked_notes_rest_nearer_each_other_than_unlinked_ones() {
        let graph = Graph::build(&vault(60));
        let layout = graph.layout();
        let linked: f32 = graph.edges.iter().map(|&(a, b)| layout[a].distance(layout[b])).sum::<f32>() / graph.edges.len() as f32;
        let mut all = 0.0;
        for a in 0..layout.len() {
            for b in a + 1..layout.len() {
                all += layout[a].distance(layout[b]);
            }
        }
        let all = all / (layout.len() * (layout.len() - 1) / 2) as f32;
        assert!(linked < 0.8 * all, "linked {linked}, any two {all}");
    }

    #[test]
    fn odd_vaults_still_get_a_layout() {
        assert_eq!(Graph::build(&notes(&[("A.md", "")])).layout(), [Vec3::ZERO]);
        let same = Graph::build(&notes(&[("A.md", "[[B]]"), ("B.md", "[[A]]")]));
        let layout = same.layout();
        assert!(layout[0].distance(layout[1]) >= same.radius(0) + same.radius(1));
        let large = Graph::build(&vault(700)).layout();
        // Keeping every disc clear of the next lets a crowded ball swell a little past its size.
        assert!(large.iter().all(|at| at.length().is_finite() && at.length() <= MAX_EXTENT * 1.25));
    }

    #[test]
    fn edges_bend_away_from_the_middle() {
        let (a, b) = (Vec3::new(100.0, 0.0, 50.0), Vec3::new(0.0, 100.0, 50.0));
        let bend = control(a, b);
        assert!(bend.length() > ((a + b) * 0.5).length());
        assert_eq!(curve(a, bend, b, 0.0), a);
        assert_eq!(curve(a, bend, b, 1.0), b);
        let through = control(Vec3::new(-50.0, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0));
        assert!(through.length() > 1.0 && through.length().is_finite(), "an edge through the middle bends too");
    }

    #[test]
    fn notes_found_are_gathered_close_about_the_middle_without_touching() {
        let graph = Graph::build(&vault(60));
        let rest = graph.layout();
        let found = [3, 17, 40, 41, 59, 8];
        let gathered = graph.gather(&rest, &found);
        assert_eq!(gathered.len(), found.len());
        assert_eq!(gathered, graph.gather(&rest, &found), "the same every time");
        let middle = gathered.iter().fold(Vec3::ZERO, |sum, &at| sum + at) * (1.0 / found.len() as f32);
        assert!(middle.length() < 0.01, "about the middle: {middle:?}");
        let spread = |at: &[Vec3]| at.iter().map(|a| at.iter().map(|b| a.distance(*b)).fold(0.0, f32::max)).fold(0.0, f32::max);
        let resting: Vec<Vec3> = found.iter().map(|&node| rest[node]).collect();
        assert!(spread(&gathered) < spread(&resting), "closer together than where they rest");
        for a in 0..found.len() {
            for b in a + 1..found.len() {
                let room = graph.radius(found[a]) + graph.radius(found[b]);
                assert!(gathered[a].distance(gathered[b]) > room, "{a} and {b} touch");
            }
        }
    }

    #[test]
    fn of_the_notes_found_the_linked_ones_are_held_a_link_apart() {
        let list = notes(&[("A.md", "[[B]]"), ("B.md", ""), ("C.md", ""), ("D.md", ""), ("E.md", "[[D]]")]);
        let graph = Graph::build(&list);
        let gathered = graph.gather(&graph.layout(), &[0, 1, 2, 3, 4]);
        for (a, b) in [(0, 1), (3, 4)] {
            let (apart, link) = (gathered[a].distance(gathered[b]), graph.radius(a) + graph.radius(b) + LINK_GAP);
            assert!((apart - link).abs() < 0.25 * link, "{a} and {b} are {apart} apart, a link being {link}");
        }
        assert_eq!(graph.gather(&graph.layout(), &[2]), [Vec3::ZERO], "one note found goes in the middle");
        assert!(graph.gather(&graph.layout(), &[]).is_empty());
    }
}
