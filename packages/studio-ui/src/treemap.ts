/**
 * Squarified treemap layout (#3339 Size view) — Bruls/Huizing/van Wijk.
 *
 * Pure geometry: values in, rects out, deterministic. The classic
 * algorithm lays each run of items along the container's SHORT side,
 * accepting an item into the run while it improves the run's worst aspect
 * ratio — which is what keeps blocks square-ish instead of slivered.
 */

export interface TreemapRect {
  key: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export function squarify(
  items: readonly { key: string; value: number }[],
  x: number,
  y: number,
  w: number,
  h: number,
): TreemapRect[] {
  const positive = items.filter((i) => i.value > 0);
  const total = positive.reduce((s, i) => s + i.value, 0);
  if (total <= 0 || w <= 0 || h <= 0) return [];
  // Sort descending — the algorithm assumes it; ties keep input order for
  // determinism (stable sort).
  const sorted = [...positive].sort((a, b) => b.value - a.value);
  const area = (w * h) / total;

  const out: TreemapRect[] = [];
  let rx = x;
  let ry = y;
  let rw = w;
  let rh = h;
  let row: { key: string; area: number }[] = [];

  const worst = (candidate: { area: number }[]): number => {
    const side = Math.min(rw, rh);
    const sum = candidate.reduce((s, i) => s + i.area, 0);
    if (sum === 0) return Infinity;
    const rowThickness = sum / side;
    let max = 0;
    for (const item of candidate) {
      const length = item.area / rowThickness;
      max = Math.max(max, rowThickness / length, length / rowThickness);
    }
    return max;
  };

  const layoutRow = (): void => {
    const side = Math.min(rw, rh);
    const sum = row.reduce((s, i) => s + i.area, 0);
    const thickness = sum / side;
    let along = 0;
    for (const item of row) {
      const length = item.area / thickness;
      if (rw >= rh) {
        out.push({ key: item.key, x: rx, y: ry + along, w: thickness, h: length });
      } else {
        out.push({ key: item.key, x: rx + along, y: ry, w: length, h: thickness });
      }
      along += length;
    }
    if (rw >= rh) {
      rx += thickness;
      rw -= thickness;
    } else {
      ry += thickness;
      rh -= thickness;
    }
    row = [];
  };

  for (const item of sorted) {
    const next = { key: item.key, area: item.value * area };
    if (row.length === 0 || worst([...row, next]) <= worst(row)) {
      row.push(next);
    } else {
      layoutRow();
      row.push(next);
    }
  }
  if (row.length > 0) layoutRow();
  return out;
}
