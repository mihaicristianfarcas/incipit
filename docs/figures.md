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
