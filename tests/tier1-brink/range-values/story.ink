// NS-A5 (issue #1111, docs/stdlib-spec.md §7): ranges as a real Value kind
// (F7) + the inhabited-range refinement's draw verb. Deterministic under
// the pinned draw algorithm + DotNetRng (the corpus harness's generator):
// seed(11) fixes every draw, so the expected.txt values are stable
// oracle-free goldens — any drift in the draw chain or the range ops'
// semantics (display form, content equality, iteration, index) breaks
// this case loudly. That is the point.
~ seed(11)
~ temp die = 1..=6
~ temp span = 0..10
forms: {string(die)} and {string(span)}
content equal: {die == 1..7}, empties: {3..3 == 8..8}
len: {len(die)} {len(span)} {len(2..2)}
index: {span[0]} {span[9]}
~ temp acc = ""
~ {
  for i in 2..=5 {
    acc = acc + " " + string(i)
  }
  for j in 0..0 {
    acc = acc + " never"
  }
}
iterated:{acc}
rolls: {int(die)} {int(die)} {int(die)} {int(0..100)}
picks: {string(pick(10..13))} {string(pick(4..4))}
validator: {string(non_empty(die))} {string(non_empty(9..=2))}
truthy: {2..3: yes|no}, empty: {0..0: yes|no}
-> END
