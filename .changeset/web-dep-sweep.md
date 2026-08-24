---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Web dependency sweep (rides the desktop-perf measure-first work): vite
6.4 → 8.2 and @vitejs/plugin-react 4.7 → 6.1 across the workspace's dev
servers/builds, Playwright 1.58 → 1.62, vitest/@types/node current, and
current minors for the runtime dependencies the published bundles carry —
the CodeMirror 6 packages (state 6.7, view 6.43, language/lint/search/
commands/autocomplete), zustand 5.0.15, @floating-ui/react-dom,
@xyflow/react, @dagrejs/dagre, @fontsource/jetbrains-mono,
react-resizable-panels, and the react 19.2.x patch line. No API changes;
the perf scenario suite was re-recorded on the new toolchain and compared
against the pre-sweep baseline (docs/desktop-perf-baseline.md).
Deliberately NOT taken: TypeScript 7 and @changesets/cli 3 (majors held
for their own decisions).
