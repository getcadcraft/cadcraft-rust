# Assets

Every file under `assets/` has a row here (author, source, licence). `cargo xtask assets` fails otherwise.
Prefer art generated in code.

| File | Author | Source | Licence |
|---|---|---|---|
| assets/samples/base-plan.dwg | CadCraft | CadCraft's own public test fixture (getcadcraft.com `fixtures/dwg/base-plan.dwg`, also in the public app repos) | Apache-2.0 |
| assets/fonts/Inter-Regular.ttf | The Inter Project Authors (rsms) | https://github.com/rsms/inter v4.1 release | OFL-1.1 (assets/fonts/Inter-OFL.txt) |
| assets/fonts/Inter-OFL.txt | The Inter Project Authors | Inter v4.1 release | OFL-1.1 text |
| assets/fonts/JetBrainsMono-Regular.ttf | JetBrains | https://github.com/JetBrains/JetBrainsMono v2.304 release | OFL-1.1 (assets/fonts/JetBrainsMono-OFL.txt) |
| assets/fonts/JetBrainsMono-OFL.txt | JetBrains | JetBrains Mono v2.304 release | OFL-1.1 text |
| assets/samples/kitchen-equipment.dwg | CadCraft | CadCraft's own public test fixture (getcadcraft.com `fixtures/dwg/kitchen-equipment.dwg`, also in the public app repos; links base-plan.dwg) | Apache-2.0 |
| assets/samples/bimcraft-house.ifc | CadCraft | CadCraft's own public showcase model (getcadcraft.com `fixtures/showcase/bimcraft-house.ifc`), the 3D IFC sample | Apache-2.0 |
| apps/cadcraft-web/favicon.svg | CadCraft | drawn for this repo (a pick box on the brand blue) | Apache-2.0 |
| assets/lang/cadcraft/en.json | CadCraft | CadCraft 0.5's own language catalog (`public_html/assets/apps/cadcraft/dev/lang/en.json`, frozen at 594ab3e), unchanged | Apache-2.0 |
| assets/lang/cadcraft/es-MX.json | CadCraft | CadCraft 0.5's own language catalog (`…/lang/es-MX.json`, frozen at 594ab3e; ships as draft), unchanged | Apache-2.0 |
| assets/lang/modelcraft/en.json | CadCraft | ModelCraft 0.5's own language catalog (`public_html/assets/apps/modelcraft/dev/lang/en.json`, frozen at 594ab3e), unchanged | Apache-2.0 |
| assets/lang/modelcraft/es-MX.json | CadCraft | ModelCraft 0.5's own language catalog (`…/lang/es-MX.json`, frozen at 594ab3e; ships as draft), unchanged | Apache-2.0 |

Test data and fuzz seeds (outside `assets/`, listed for the record):

| File | Author | Source | Licence |
|---|---|---|---|
| crates/io-cad/tests/data/base-plan.dxf | CadCraft | CadCraft's own public test fixture (getcadcraft.com `fixtures/dwg/base-plan.dxf`); also the `cad_dxf_read` fuzz seed `fuzz/seeds/cad_dxf_read/base-plan.dxf` | Apache-2.0 |
| crates/io-cad/tests/data/save-fixture.json | CadCraft | generated from the synthetic drawing in CadCraft's `tools/cadcraft-test/dwg-save-check.mjs` (no client data) | Apache-2.0 |
| fuzz/seeds/cad_dxf_read/save-fixture-2000.dxf | CadCraft | that fixture saved as DXF 2000 by cc-io-cad | Apache-2.0 |
| fuzz/seeds/dwg_roundtrip/sink-AC10{15,21,32}.dwg | CadCraft | written by cc-io-dwg's writer from public fixture content | Apache-2.0 |
