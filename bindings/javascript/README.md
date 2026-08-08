# duallity JavaScript bindings

`@vinary-tree/duallity` owns the JavaScript, TypeScript, and ClojureScript
facade for dictionary-backed edit and phonetic WFSTs. Construction captures the
dictionary snapshot once and returns a retained lazy `vt.scalar-wfst.1`
resource. The facade delegates to `@vinary-tree/vinary-tree`, preserving O(1)
same-runtime handoff to `@vinary-tree/lling-llang` composition.
