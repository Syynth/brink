// A port of .NET `System.Random` (Knuth's subtractive generator), and the
// shim that installs it as inkjs's PRNG.
//
// inkjs ships a Park–Miller generator (`engine/PRNG.js`, "taken from
// https://gist.github.com/blixt/f17b47c62508be59987b"), while the C# ink
// runtime — the rank-2 reference behind every checked-in `*.oracle.json` —
// draws every shuffle, `RANDOM` and `LIST_RANDOM` from
// `new System.Random(seed).Next()`. Same seeds, different generators: every
// draw diverges by construction, which is the "RNG seed mapping" #3379 asked
// to prove or exclude. There is no mapping; there is a replacement. brink's
// runtime already ports this generator (`crates/brink-runtime/src/rng.rs`,
// `DotNetRng`) for the same reason, and the vectors in
// `dotnet-random.test.mjs` are that file's.
//
// Story.js reads `PRNG_1.PRNG` off the module object at every call site
// (`new PRNG_1.PRNG(seed)` then `.next()`), and `inkjs/compiler/*` is
// unbundled CommonJS sharing those engine modules — so replacing the export
// swaps the generator under the compiler-built story without patching
// inkjs itself. `inkjs/full` (the rollup bundle) inlines its own copy and
// cannot be patched this way; the oracle deliberately imports the compiler
// module path instead.

import { createRequire } from "node:module";

const MBIG = 2147483647;
const MSEED = 161803398;
const SEED_ARRAY_LEN = 56; // index 0 unused, 1..55 active

/** Wrap to a signed 32-bit integer — C#'s unchecked `int` arithmetic. */
const wrap = (n) => n | 0;

export class DotNetRandom {
  /** @param {number} seed — coerced to a C# `int` like `new Random(int)`. */
  constructor(seed) {
    seed = wrap(Math.floor(seed));
    const seedArray = new Int32Array(SEED_ARRAY_LEN);

    const subtraction = seed === -2147483648 ? 2147483647 : Math.abs(seed);
    let mj = wrap(MSEED - subtraction);
    seedArray[55] = mj;
    let mk = 1;

    for (let i = 1; i < 55; i++) {
      const ii = (21 * i) % 55;
      seedArray[ii] = mk;
      mk = wrap(mj - mk);
      if (mk < 0) mk = wrap(mk + MBIG);
      mj = seedArray[ii];
    }

    for (let k = 1; k < 5; k++) {
      for (let i = 1; i < 56; i++) {
        const idx = 1 + ((i + 30) % 55);
        seedArray[i] = wrap(seedArray[i] - seedArray[idx]);
        if (seedArray[i] < 0) seedArray[i] = wrap(seedArray[i] + MBIG);
      }
    }

    this.seedArray = seedArray;
    this.inext = 0;
    this.inextp = 21;
  }

  /** `Random.Next()` — a non-negative int32, the same sequence .NET produces. */
  next() {
    let inext = this.inext + 1;
    if (inext >= 56) inext = 1;
    let inextp = this.inextp + 1;
    if (inextp >= 56) inextp = 1;

    let num = wrap(this.seedArray[inext] - this.seedArray[inextp]);
    if (num === MBIG) num -= 1;
    if (num < 0) num = wrap(num + MBIG);

    this.seedArray[inext] = num;
    this.inext = inext;
    this.inextp = inextp;
    return num;
  }

  /** inkjs's PRNG exposes this too; nothing in Story.js calls it. */
  nextFloat() {
    return this.next() / MBIG;
  }
}

/**
 * Replace inkjs's PRNG export with {@link DotNetRandom}. Idempotent. Returns
 * the module object so a caller can assert the swap took.
 */
export function installDotNetRandom() {
  const require = createRequire(import.meta.url);
  const prngModule = require("inkjs/engine/PRNG");
  prngModule.PRNG = DotNetRandom;
  return prngModule;
}
