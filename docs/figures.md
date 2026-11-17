# Figures

A figure is a typed spec a model fills in: no coordinates, no code. The
engine (`crates/incipit-scene`) lays it out, animates it and checks the
drawing in many states before anyone sees it. What fails comes back as
sentences, which go to the model as they are.

The engine prevents rather than detects where it can: it computes every
position, value and label place, so whole classes of error cannot happen.
The checks catch what is left.

## The parts

| Part | What it is |
|---|---|
| `Figure` | The spec, one variant per kind, tagged by `kind` in JSON |
| `State` | What changes while a learner looks: the clock, the step, parameter values, points moved, an element in focus |
| `layout(figure, state, measure)` | The `Frame` for one state: marks to paint, the narration, the handles, and the issues the layout ran into |
| `Handle` | A control described as data: a `Slider` for a parameter, or a `Drag` (a point, the stage box it is held in, and what moving it sets). `State::drag` does the arithmetic, so a window wires handles without knowing the kind |
| `check(figure, measure)` | A `Report` of errors (the figure must not be shown) and warnings |
| `schema::schema(kind)`, `schema::guide(kind)`, `schema::example(kind)` | What a model is given for a kind: a strict JSON Schema (every field required, nothing extra), the words that go with it, and a small example that passes every check |

Marks are lines, filled shapes, boxes, circles and text on a stage of
640 × 400. Colours are tones (`hi`, `mid`, `lo`, `faint`, `lit`, `red`,
`green`, `blue`) the window's theme resolves. Text is measured through
`Measure`, so labels are placed by the widths the window really draws;
`Rough` measures by rule of thumb where there is no window.

## The checks

First the spec's own sense, from each kind's `structure`: ids that are
well formed and used once, references to things that exist, ranges that
make sense, sizes a figure can hold.

Then, if the spec makes sense, the drawing in every state of the kind's
`samples`: every step, moments on a clock, each parameter at its minimum,
middle and maximum and their corners, each draggable point at the edges of
its region. In each state:

- every mark is inside the stage and every number is finite;
- no text is smaller than 9 units;
- no text overlaps other text or a box that holds words;
- a line of one element crossing another's words is a warning;
- whatever the layout reported (a label that does not fit its box, a point
  that is not defined) is an error or a warning.

A problem is reported once, with the first state it was seen in: "the text
"A" overlaps the text "B" (with a = 2)".

Labels are placed by `labels::Labels`: each label tries the spots its
layout offers in order and takes the first clear of what is reserved
(boxes, earlier labels, lines). Text is never cut: `labels::wrap` breaks a
label at spaces, and says when it cannot fit, which becomes an error
naming the label.

## The kinds

### Sequence

Who tells whom what, over time. Actors stand on lifelines; messages travel
between them while a clock runs down the page; states, busy spans,
crashes and recoveries sit on the lifelines.

Spec: `actors` (id, label, sublabel, shape) and `events` (message, state,
span, crash, recover), each with times and a narration. It is the one kind
on a clock: `State::time`.

Checks: 2 to 6 actors, at most 24 events, events naming actors that
exist, arrivals after sends; the drawing at every event's start and end
and halfway between.

### Flow

Boxes and arrows: processes, causes and effects, decisions, state
machines, cycles, hierarchies and trees.

Spec: `arrange` (`down`, `right` or `ring`), `nodes` (id, label, shape:
box, round, decision, terminal or circle; tone), `edges` (from, to, label,
tone, dashed) and `steps`.

Layout (`flow/layers.rs`, `flow/layout.rs`): a layered layout in the
Sugiyama manner. A depth-first search in the spec's order turns back the
edges that close a loop; longest-path layering, with each source moved
next to what it feeds; a placeholder in each layer a long edge passes;
barycentre sweeps that keep the order with the fewest crossings; then
each layer placed as near as it can to its neighbours while keeping its
order and gaps (an isotonic regression, solved exactly). Edges leave and
enter through ports spread along a node's side, run straight across the
gaps between layers and through their placeholders' slots, so no edge can
pass through a box. `ring` sets nodes clockwise around an ellipse and
draws edges between neighbours as arcs. Labels wrap inside their shape;
edge labels go beside their line, clear of boxes and other lines.

Checks: 2 to 16 nodes, at most 24 edges, at most 8 on a ring, edges and
steps that name real elements; a label that does not fit its shape, too
many nodes side by side, and too many layers are errors that say what to
change.

### Plot

Functions of x and data the learner plays with: rates, growth, waves,
distributions (a density is a curve, a probability an area under it),
paths and vectors, and the numbers a text gives.

Spec: `x` and `y` axes (a label and a range; a `y` range of 0 to 0 lets
the engine choose it from what is drawn), `equal` (one unit as long
across as up, for geometry: the engine widens the tighter range),
`params` (name, label, range and start: each is a slider), `layers` and
`steps`. A layer is a `curve`, an `area` between two functions, a
`point`, a `tangent` to a curve, a vertical or horizontal line, a
`parametric` path (x and y as s runs between two values), a `series` of
numbers from the text (dots, a line or both), `bars` (a height at each
place, with names under them in place of x's numbers, or none), or a
`vector` (an arrow between two points). A point whose x or y is one
parameter's name can be dragged, which sets that parameter. A step may
set parameters.

Every number that moves is an expression over x (s in a path) and the
parameters (`expr.rs`): arithmetic, comparisons and a fixed list of functions,
parsed into a tree and evaluated. It is never run as code, it has no
loops and no names but the ones the spec declares, and its length and
nesting are limited.

Layout (`plot/layout.rs`, with paths, series, bars and vectors in
`plot/layers.rs`): curves and paths are sampled across the plot, broken
where they are undefined or jump (an asymptote) and clipped to the plot;
an automatic y range holds every data point, bar and arrow, and leaves
out only the most extreme values of sampled curves; the ticks are chosen
from the range (`plot/axes.rs`); labels are placed last, clear of each
other.

Checks: at most 4 parameters and 1 to 8 layers; ranges with a min below
their max; expressions that parse and use only known names; a dragged
point's coordinate that is one parameter; steps that set real parameters
inside their range; a series of 1 to 2000 points drawn as dots, a line or
both; 1 to 60 bars with a height for each and a name for each or none.
The drawing is then looked at across the parameters' ranges and the
steps, and a point, a data point, a bar or an arrow's end that leaves
the plot is an error that says where and with which values; names under
bars that run into each other are refused like any text that overlaps.

### Graph

A network: nodes linked in any pattern (computers, peers, servers,
people, the slots of an array), with tokens that travel along the links.
Layered graphs and trees are the flow kind's.

Spec: `layout` (`force`, `circle`, `row` or `grid`), `nodes` (id, label,
sublabel, shape: circle, box, pill, diamond, database or cell; tone),
`edges` (from, to, label, tone, dashed, arrow), `tokens` (a path of node
ids, the second it leaves, the seconds each hop takes, a label), `groups`
(a label and the nodes it boxes) and `steps`. A graph with tokens plays
on a clock that runs until the last arrives; one without may have steps.

Layout (`graph/place.rs`, `graph/layout.rs`), as the earlier app set
graphs out: each node is sized to its label; `force` runs a fixed
simulation in the manner of d3-force (links pull to a length, every pair
pushes apart, nodes keep clear of each other, a weak pull to the middle;
it starts from an ellipse and runs a set number of ticks, so it is the
same every time); `circle` sets the nodes clockwise from the top, `row`
left to right, `grid` in rows; the positions are fitted to the stage and
nodes that still overlap are pushed apart. Edges run between the nodes'
outlines; two between one pair bend to their own left, so a pair joined
both ways bends apart; a node's link to itself loops over it. Tokens
ride along the edges, easing in and out of each hop. A drawing larger
than the stage is shrunk about its middle, text and all.

Checks: 2 to 16 nodes, at most 96 edges, 24 tokens and 12 groups; edges,
tokens and groups that name real nodes; tokens with a path of two nodes
or more that leave at 0 seconds or later and take at least 0.05 seconds
a hop; tokens and steps not both. In the drawing: a node label wider
than its circle (or 130 units for other shapes), an edge label with no
room, a group's box that takes in a node not in it, and text shrunk too
small to read are errors. The drawing is looked at in each step, or at
each moment a token leaves or reaches a node and halfway between.

### Diagram

A side view of a physical setup that moves, the drawing a teacher makes
on a blackboard: a pendulum, a mass on a spring, a block on a slope with
the forces on it, a gas in a box.

Spec: `params` (as a plot's), `seconds` (how long the clock runs, 0 for
none), `parts` and `steps`. A part is a `support` (a floor, a ceiling or a
wall, by the side things are on), a `rod`, a `string`, a `spring`, a
`block`, a `ball`, a `ramp`, an `arrow` (a force or a velocity) or
`particles` bouncing inside a block. The first part is on nothing; every
other part is `on` one listed before it, `at` a distance along it, with
its own lengths and angles in the setup's units. Nothing gives a
position: a body on a floor, a slope or a block's top sits on it,
touching; a body on a rod, string or spring is centred on its point (at
`end`, its far end); a rod, string, spring or arrow on a body starts from
its middle. Every length, angle and distance is an expression over the
parameters, `t` (the clock's seconds) and `end` (the length of what the
part is on), so a pendulum swings because its string's angle is a
function of `t`. A block or ball whose `at` is one parameter's name can
be dragged along what it is on. A diagram has a clock or steps, not both.

Layout (`diagram/solve.rs`, `diagram/layout.rs`, and the drawing's own
checks in `diagram/collide.rs`): each part is placed from
the part it is on (`solve`), then the whole setup is fitted to the stage
once, over every state the checks look at, one unit as long across as up,
so it neither jumps nor leaves the stage while it moves; the fit is kept
for the spec, so a frame does not refit. Supports reach just past all that
hangs from them (one standing on another starts at its foot) and are
hatched on their solid side; springs are drawn as coils; bodies are drawn
over what they hang from; labels go beside their parts, clear of the rest.

Checks: 1 to 24 parts, one of them first and every other on a part that
exists, with no loop; each part on something it can be on (a ramp on a
floor, particles in a block, nothing on an arrow); at most 4 parameters
with ranges; expressions that parse and use only the names they may (`t`
only with a clock, `end` only in `at` along a rod, string, spring, ramp
or block); 1 to 200 particles; a clock of up to 600 seconds. In the
drawing, in each step or at 48 moments of the clock, each with the
parameters at their ends and middle: a value that is not a number, a
part past the end of what it is on, a length or size of 0 or less (an
arrow may turn round), a ramp steeper than 80 degrees or flatter than 5,
a body or a rod too small to see, bodies that run into each other or a
ramp or go into a support's solid side, and a label with no room are
errors.

## Steps

Flow, graph, plot and diagram share `steps::Step` (a plot's and a
diagram's steps also set parameters): one sentence of
narration, the ids that appear at that step (`show`) and the ids to light
(`focus`). An element no step shows is there from the start. Positions
never depend on what is shown, so nothing jumps as the learner steps on;
only what a parameter a step sets moves. The
checks refuse a step that names an unknown id, shows an element twice, or
lights one before it appears.

## Adding a kind

1. A module `crates/incipit-scene/src/<kind>/` with the spec types (serde,
   `deny_unknown_fields`), `layout`, the structure checks and samples, and
   `schema.rs` with the schema, the guide and an example. Split by concern
   so no file passes about 800 lines.
2. `impl Spec for` the spec type.
3. A variant in `Figure` and `Kind`, and the kind's arms in
   `schema::{schema, guide, example}`.
4. Tests: the example passes its checks (the test in `schema.rs` covers
   every kind), and each structure rule and layout guarantee has its own.
5. Look at the example on the Figure page (`INCIPIT_KIND=<kind>`), in a
   few states, and try the schema and guide with a model.
