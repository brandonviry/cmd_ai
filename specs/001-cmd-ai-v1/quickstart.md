# Quickstart (validation de la feature)

```powershell
cargo build --release
# Dossier portable de test
New-Item -ItemType Directory -Force demo | Out-Null
Copy-Item target\release\cmd_ai.exe demo\
Copy-Item portable\config.example.json demo\config.json   # puis renseigner provider.api_key
.\demo\cmd_ai.exe --version
.\demo\cmd_ai.exe --prompt "affiche la date et l'heure" --no-tui
.\demo\cmd_ai.exe            # TUI interactif
```

Résultat attendu : le plan s'affiche, « o » l'exécute, la commande et sa sortie apparaissent avec `1/1`, puis la réponse du modèle ; code de sortie 0 ; un fichier de session apparaît dans `demo\log\`.

Tests : `cargo test` (aucun réseau, aucune clé requise).
