---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

`FileProvider.renameFile` now receives the moved file's rewritten source
(issue #2425).

The rename op folds the moved file's own outbound `INCLUDE` rewrites into
`new_source`, but the atomic-rename branch of `ProjectSession.renameFile`
passed only the two paths on — so a host whose `renameFile` moves bytes
(a real filesystem rename) kept the pre-rewrite text in storage, while the
`createFile` + `deleteFile` fallback branch had always written the rewritten
source. The optional third parameter, `newContent`, closes that gap:

```ts
renameFile?(oldPath: string, newPath: string, newContent?: string): Promise<void>;
```

It is optional and additive — an existing implementation declaring only
`(oldPath, newPath)` still satisfies the interface and behaves exactly as
before. `InMemoryFileProvider` now stores `newContent` when supplied.

`@brink-lang/studio` re-exports `FileProvider` through `mountStudio`'s
`MountStudioOptions.provider` (`packages/brink-studio/src/mount.tsx`), so an
embedder that supplies its own provider and implements `renameFile` is
affected by the new third argument: it can keep taking two parameters and
see no change, or add the third to persist the rewritten source the way
`InMemoryFileProvider` and `TauriFileProvider` now do.
