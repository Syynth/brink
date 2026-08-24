/**
 * `?fixture=perf` — the synthetic studio-scale project for performance
 * measurement (measure-first ruling, docs/decision-log.md 2026-08-24).
 *
 * Deterministic (fixed-seed LCG, no wall clock, no entropy): every run, on
 * every machine, generates byte-identical files, so recorded perf runs
 * (`perf-runs/`) are comparable across attempts and across code changes.
 * The shape mirrors `compile_bench`/`ide_bench`'s Rust generator — 50
 * included files × 20 knots cycling four content templates — so browser
 * numbers sit next to the native rows, PLUS one deliberately large file
 * (`large.ink`, ~8k lines) reproducing the reported large-file symptom
 * (typing latency on a line break; scrolling ahead of CM6's rendered
 * viewport). Byte-parity with the Rust generator is NOT a goal (the two
 * version independently); the SHAPE parity is what makes rows comparable.
 */

const SYN_FILES = 50;
const SYN_KNOTS = 20;
/** Knots in `large.ink` — at ~8 lines per knot this yields ~8k lines. */
const LARGE_KNOTS = 900;

const WORDS = [
  "lantern", "harbor", "signal", "vault", "ember", "cipher", "meadow", "static", "orchard",
  "beacon", "drift", "hollow", "ledger", "murmur", "quarry", "relay", "sable", "tundra",
  "vesper", "wharf", "zenith", "gable", "isthmus", "keel",
];

class Lcg {
  private state = 0x5eed0498n;

  next(): number {
    // 64-bit LCG step via BigInt, masked to 64 bits; upper bits are the
    // usable stream (same constants as the Rust twin).
    this.state =
      (this.state * 6364136223846793005n + 1442695040888963407n) & 0xffffffffffffffffn;
    return Number((this.state >> 33n) & 0x7fffffffn);
  }

  pick(lo: number, hi: number): number {
    return lo + (this.next() % (hi - lo + 1));
  }
}

function sentence(rng: Lcg, minWords: number, maxWords: number): string {
  const n = rng.pick(minWords, maxWords);
  const words: string[] = [];
  for (let i = 0; i < n; i++) {
    const w = WORDS[rng.pick(0, WORDS.length - 1)];
    words.push(i === 0 ? w.charAt(0).toUpperCase() + w.slice(1) : w);
  }
  return `${words.join(" ")}.`;
}

function generateKnot(
  rng: Lcg,
  name: string,
  next: string,
  template: number,
  varPrefix: string,
): string {
  let s = `=== ${name} ===\n`;
  switch (template) {
    case 0: {
      const lines = rng.pick(3, 5);
      for (let i = 0; i < lines; i++) s += `${sentence(rng, 5, 11)}\n`;
      s += "-> DONE\n";
      break;
    }
    case 1:
      s += `${sentence(rng, 4, 8)}\n`;
      s += `~ ${varPrefix}_0 = ${varPrefix}_0 + 1\n`;
      s += `The counter reads {${varPrefix}_0} at this point.\n`;
      s += `${next}\n`;
      break;
    case 2:
      s += `${sentence(rng, 4, 8)}\n`;
      s += `* [${sentence(rng, 2, 4)}]\n`;
      s += `    ${sentence(rng, 4, 8)}\n`;
      s += `* [${sentence(rng, 2, 4)}]\n`;
      s += `    ${sentence(rng, 4, 8)} # aside\n`;
      s += `- ${sentence(rng, 3, 6)}\n`;
      s += `${next}\n`;
      break;
    default:
      s += `{${varPrefix}_1 > 4: ${sentence(rng, 3, 5)}|${sentence(rng, 3, 5)}}\n`;
      s += `${next}\n`;
      break;
  }
  return `${s}\n`;
}

function generateFile(f: number): string {
  const rng = new Lcg();
  for (let i = 0; i <= f; i++) rng.next();
  const id = String(f).padStart(2, "0");
  const varPrefix = `var_f${id}`;
  let s = `// Generated file ${id}.\n`;
  for (let v = 0; v < 3; v++) s += `VAR ${varPrefix}_${v} = ${rng.pick(0, 9)}\n`;
  s += "\n";
  for (let k = 0; k < SYN_KNOTS; k++) {
    const kid = String(k).padStart(2, "0");
    const nextKid = String(k + 1).padStart(2, "0");
    const next = k + 1 < SYN_KNOTS ? `-> k${id}_${nextKid}` : "-> DONE";
    s += generateKnot(rng, `k${id}_${kid}`, next, (f * SYN_KNOTS + k) % 4, varPrefix);
  }
  return s;
}

/** The large-file half of the fixture: one file, ~8k lines, real content
 *  mix — the scroll/typing symptom reproducer. */
function generateLargeFile(): string {
  const rng = new Lcg();
  let s = "// large.ink — the large-file symptom reproducer (~8k lines).\n";
  s += "VAR large_0 = 0\nVAR large_1 = 5\n\n";
  for (let k = 0; k < LARGE_KNOTS; k++) {
    const kid = String(k).padStart(3, "0");
    const nextKid = String(k + 1).padStart(3, "0");
    const next = k + 1 < LARGE_KNOTS ? `-> big_${nextKid}` : "-> DONE";
    s += generateKnot(rng, `big_${kid}`, next, k % 4, "large");
  }
  return s;
}

/** Generate the whole `?fixture=perf` project as `path → source`. */
export function generatePerfFixture(): Record<string, string> {
  const files: Record<string, string> = {};
  for (let f = 0; f < SYN_FILES; f++) {
    files[`file_${String(f).padStart(2, "0")}.ink`] = generateFile(f);
  }
  files["large.ink"] = generateLargeFile();

  let main = "// Synthetic studio-scale perf project — generated, deterministic.\n";
  for (let f = 0; f < SYN_FILES; f++) {
    main += `INCLUDE file_${String(f).padStart(2, "0")}.ink\n`;
  }
  main += "INCLUDE large.ink\n";
  main += "VAR main_counter = 0\n";
  main += "The opening line of the synthetic perf project. # generated\n";
  main += "~ main_counter = main_counter + 1\n";
  main += "-> k00_00\n";
  files["main.ink"] = main;
  return files;
}
