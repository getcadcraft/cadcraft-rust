# Golden references (public fixtures only)

`js-0.5/<engine>/<fixture>.(json|ndjson).gz` — the CadCraft 0.5 JavaScript engines' output (frozen at site commit 594ab3e)
for the public fixtures, in the parity kit's canonical form, made with the kit:

    cd getcadcraft.com && node tools/fork-parity/ref.mjs --public-out <repo>/tests/golden/js-0.5
    find tests/golden/js-0.5 -type f -name '*.json' -o -name '*.ndjson' | xargs gzip -n -9

`fixtures/` — the public fixtures they were made from (CadCraft's own sample files from getcadcraft.com `fixtures/dwg`,
`fixtures/electrical`, `fixtures/folder`, `fixtures/showcase`; Apache-2.0 like the rest of CadCraft). `base-plan.dwg`
and `kitchen-equipment.dwg` are read from `assets/samples/`.

The test `apps/cadcraft-cli/tests/golden.rs` runs `cadcraft-cli parity <engine> <fixture>` and compares with the kit's
rule (keys in any order, numbers within the engine's tolerance: dwg abs 1e-9 + rel 1e-12, ifc-step exact, ifc-geom
1e-6). Engines not ported yet are skipped. The private corpus never enters this repo; the full check is the kit's
`run.mjs` on the build server.
