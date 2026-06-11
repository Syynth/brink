/**
 * Chromium-88 `document.adoptedStyleSheets` compatibility shim (issue #154).
 *
 * Why this exists: NW.js as shipped by RPG Maker MZ runs Chromium 88, where
 * `document.adoptedStyleSheets` is a frozen `FrozenArray<CSSStyleSheet>` —
 * reassignment works, but in-place mutation throws. CodeMirror's style-mod
 * `push()`es new sheets into it, so mounting the studio there dies with
 * `TypeError: Cannot add property 0, object is not extensible`. Modern
 * browsers expose a mutable `ObservableArray`, where `push()` is fine.
 *
 * The shim (mirroring the working host-side shim from
 * `celeris/rmmz/packages/brink-studio-plugin`, moved studio-side so every
 * old-engine host doesn't rediscover it): when — and only when — the frozen
 * shape is detected, the document instance gets an own `adoptedStyleSheets`
 * accessor serving a mutable wrapper array whose mutations sync through the
 * native *assignment* setter (which Chromium 88 does support). Modern
 * browsers fail the feature-detect and take zero overhead — the property is
 * never touched.
 */

// Minimal structural view of a document, so the detector/installer are unit
// testable against a fake host (jsdom doesn't implement adoptedStyleSheets).
export interface AdoptedSheetsHost {
  adoptedStyleSheets?: unknown;
}

/**
 * Feature-detect the Chromium-88 shape: an actual (frozen) Array rather
 * than a mutable ObservableArray. Pure — pass `document.adoptedStyleSheets`.
 */
export function adoptedStyleSheetsNeedShim(sheets: unknown): boolean {
  return Array.isArray(sheets) && !Object.isExtensible(sheets);
}

/**
 * Install the wrapper on `host` when needed. Returns true when installed,
 * false when the environment is modern (mutable array), lacks
 * adoptedStyleSheets entirely (nothing to fix — style-mod falls back to
 * <style> tags), or has no reachable native accessor pair.
 */
export function installAdoptedStyleSheetsShim(host: AdoptedSheetsHost = document): boolean {
  let current: unknown;
  try {
    current = host.adoptedStyleSheets;
  } catch {
    return false;
  }
  if (!adoptedStyleSheetsNeedShim(current)) return false;

  // Find the native accessor pair on the prototype chain (Document.prototype
  // in Chromium 88) — the wrapper syncs through its setter.
  let proto: object | null = Object.getPrototypeOf(host);
  let descriptor: PropertyDescriptor | undefined;
  while (proto !== null && descriptor === undefined) {
    descriptor = Object.getOwnPropertyDescriptor(proto, "adoptedStyleSheets");
    if (descriptor === undefined) proto = Object.getPrototypeOf(proto);
  }
  if (descriptor?.get === undefined || descriptor.set === undefined) return false;
  const nativeSet = descriptor.set;

  // The mutable wrapper: a plain array behind a Proxy. Every mutation
  // (push sets an index + length; splice/pop similar) lands in a trap,
  // which forwards a fresh copy through the native setter. Intermediate
  // states during multi-step mutations are always valid sheet arrays, so
  // the extra setter calls are harmless (sheet counts are tiny).
  const shadow = [...(current as unknown[])];
  const sync = (): void => {
    try {
      nativeSet.call(host, [...shadow]);
    } catch {
      // A bad element (e.g. a non-CSSStyleSheet) would throw natively too —
      // swallow so the wrapper itself never breaks callers mid-mutation.
    }
  };
  const wrapper = new Proxy(shadow, {
    set(target, property, value) {
      const ok = Reflect.set(target, property, value);
      if (ok) sync();
      return ok;
    },
    deleteProperty(target, property) {
      const ok = Reflect.deleteProperty(target, property);
      if (ok) sync();
      return ok;
    },
  });

  Object.defineProperty(host, "adoptedStyleSheets", {
    configurable: true,
    get: () => wrapper,
    set: (value: unknown) => {
      shadow.length = 0;
      shadow.push(...(value as unknown[]));
      sync();
    },
  });
  return true;
}
