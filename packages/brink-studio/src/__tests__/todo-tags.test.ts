/**
 * `TODO(tag):` tags, their chips, and the tag filter.
 *
 * The tag needs no language support: the ink parser already takes
 * everything after `TODO` to end of line, so `TODO(audio): mix this` simply
 * arrives as the note text `(audio): mix this`. Verified by probe against
 * `brink-syntax` before this was built — the whole feature is a split and a
 * filter.
 */
import { describe, expect, it } from "vitest";
import {
  filterTodosByTag,
  splitTodoTag,
  todoTags,
  type TodoItem,
} from "@brink/studio-ui";
import { createStudioStore, loadTodosPrefs } from "@brink/studio-store";

const item = (text: string, tag: string | null, file = "a.ink"): TodoItem => ({
  file,
  start: 0,
  end: 1,
  text,
  tag,
  line: 1,
  container: null,
});

describe("splitTodoTag", () => {
  it("lifts a leading (tag) off the note", () => {
    expect(splitTodoTag("(audio): mix this down")).toEqual({
      tag: "audio",
      text: "mix this down",
    });
  });

  it("accepts a tag with no colon after it", () => {
    expect(splitTodoTag("(audio) mix this down")).toEqual({
      tag: "audio",
      text: "mix this down",
    });
  });

  it("trims space inside the parens", () => {
    expect(splitTodoTag("( audio ): x")).toEqual({ tag: "audio", text: "x" });
  });

  it("leaves an untagged note alone", () => {
    expect(splitTodoTag("mix this down")).toEqual({ tag: null, text: "mix this down" });
  });

  it("does not treat an empty paren pair as a tag", () => {
    // An empty chip would be unlabelled and unfilterable, so the parens
    // stay as written text.
    expect(splitTodoTag("(): x")).toEqual({ tag: null, text: "(): x" });
    expect(splitTodoTag("(   ) x")).toEqual({ tag: null, text: "(   ) x" });
  });

  it("only takes a tag at the head, not mid-sentence", () => {
    expect(splitTodoTag("mix (audio) down")).toEqual({ tag: null, text: "mix (audio) down" });
  });

  it("keeps parens that are part of the prose after the tag", () => {
    expect(splitTodoTag("(audio): fade (slowly)")).toEqual({
      tag: "audio",
      text: "fade (slowly)",
    });
  });
});

describe("todoTags", () => {
  it("lists each tag once, in first-appearance order", () => {
    const items = [item("a", "audio"), item("b", "art"), item("c", "audio"), item("d", null)];
    expect(todoTags(items)).toEqual(["audio", "art"]);
  });

  it("is empty when nothing is tagged", () => {
    expect(todoTags([item("a", null)])).toEqual([]);
  });

  it("treats differently-cased tags as distinct", () => {
    // Shown as written rather than folded: merging `Audio` into `audio`
    // would display a tag the author never typed.
    expect(todoTags([item("a", "Audio"), item("b", "audio")])).toEqual(["Audio", "audio"]);
  });
});

describe("filterTodosByTag", () => {
  const items = [item("a", "audio"), item("b", "art"), item("c", null)];

  it("shows everything when no tag is selected", () => {
    expect(filterTodosByTag(items, [])).toHaveLength(3);
  });

  it("keeps only the selected tag", () => {
    expect(filterTodosByTag(items, ["audio"]).map((i) => i.text)).toEqual(["a"]);
  });

  it("unions multiple selected tags", () => {
    expect(filterTodosByTag(items, ["audio", "art"]).map((i) => i.text)).toEqual(["a", "b"]);
  });

  it("drops untagged notes once any tag is selected", () => {
    // Selecting a tag is a statement about what you want to see; an
    // untagged note answers no tag.
    expect(filterTodosByTag(items, ["audio"]).some((i) => i.tag === null)).toBe(false);
  });
});

describe("the todos slice", () => {
  it("toggles a tag on and off", () => {
    const store = createStudioStore();
    store.getState().toggleTodoTag("audio");
    expect(store.getState().todosSelectedTags).toEqual(["audio"]);
    store.getState().toggleTodoTag("audio");
    expect(store.getState().todosSelectedTags).toEqual([]);
  });

  it("closing the filter row clears the selection", () => {
    // A closed filter that is still narrowing the list is a panel hiding
    // notes with no visible cause.
    const store = createStudioStore();
    store.getState().toggleTodosFilter();
    store.getState().toggleTodoTag("audio");
    expect(store.getState().todosSelectedTags).toEqual(["audio"]);
    store.getState().toggleTodosFilter();
    expect(store.getState().todosFilterOpen).toBe(false);
    expect(store.getState().todosSelectedTags).toEqual([]);
  });

  it("defaults to grouped, with the filter row closed", () => {
    const s = createStudioStore().getState();
    expect(s.todosGrouped).toBe(true);
    expect(s.todosFilterOpen).toBe(false);
    expect(s.todosSelectedTags).toEqual([]);
  });

  it("persists grouping but never the tag selection", () => {
    const written: unknown[] = [];
    const store = createStudioStore();
    store.getState().setTodosPrefsSink((p) => void written.push(p));
    store.getState().toggleTodoTag("audio");
    expect(written).toEqual([]); // a tag is not a persisted preference
    store.getState().toggleTodosGrouped();
    expect(written).toEqual([{ grouped: false }]);
  });

  it("defaults to grouped when nothing is stored", () => {
    const storage = { getItem: () => null } as unknown as Storage;
    expect(loadTodosPrefs(storage)).toEqual({ grouped: true });
  });

  it("survives a corrupt stored record", () => {
    const storage = { getItem: () => "{not json" } as unknown as Storage;
    expect(loadTodosPrefs(storage).grouped).toBe(true);
  });
});
