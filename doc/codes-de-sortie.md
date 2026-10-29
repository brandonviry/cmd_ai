---
title: Codes de sortie
description: Les codes de sortie de cmd_ai (0, 1, 2, 3, 4, 5 et 130), ce qu'ils signifient et comment les utiliser dans un script PowerShell ou shell.
---

Quand cmd_ai s'arrête, son code de sortie dit comment la tâche s'est terminée. C'est surtout utile en one-shot (`--no-tui`) et dans les scripts.

## La table

| Code | Sens | Exemples |
| --- | --- | --- |
| `0` | tâche réussie (ou TUI quitté normalement) | la demande a abouti ; vous avez quitté le TUI avec `/quitter` |
| `1` | tâche échouée | une commande en échec que l'agent n'a pas pu corriger ; plafond de 30 étapes atteint |
| `2` | erreur d'usage (arguments) | option inconnue ; `--prompt` et `--prompt-file` ensemble ; ni prompt ni terminal interactif |
| `3` | plan non validé | vous avez refusé le plan ; mode plan sans terminal pour le valider |
| `4` | erreur de configuration | `config.json` absent ou invalide ; version non prise en charge ; rôle de modèle inconnu ; dossier cmd_ai introuvable |
| `5` | erreur du fournisseur ou du réseau | clé API invalide ; crédits épuisés ; quota ; erreur du serveur ; TLS ; proxy |
| `130` | interruption | Ctrl+C ou Échap pendant la tâche |

Dans chaque cas, un message en français explique le problème et, quand c'est possible, comment le résoudre.

## Précisions

- **Code 1 et corrections.** Après un échec, l'agent tente jusqu'à 2 corrections d'une même commande. Le code 1 n'arrive que si la tâche n'a pas pu aboutir malgré tout.
- **Code 3 dans un script.** En mode plan, un script ne peut pas valider le plan : cmd_ai l'affiche et sort avec 3 sans rien exécuter. Pour qu'un script exécute, utilisez `--bypass` (voir [Modes](./modes.md)).
- **Code 4 au premier lancement.** Sans `config.json`, cmd_ai propose de le créer si le terminal est interactif ; sinon il explique la marche à suivre et sort avec 4 (voir [Installation](./installation.md)).
- **Code 5.** Les erreurs du fournisseur ont chacune leur message : clé invalide (HTTP 401), crédits épuisés (402), file d'attente ou quota (429), serveur (5xx), et réseau, proxy ou certificat. Aucune commande n'est exécutée quand le fournisseur est injoignable. Voir [Configuration](./configuration.md) pour le proxy et les certificats.

## Dans un script

### PowerShell

```powershell
& 'D:\cmd_ai\cmd_ai.exe' --no-tui --bypass --prompt "compresse le dossier photos en photos.zip"
switch ($LASTEXITCODE) {
  0       { Write-Output "Terminé." }
  3       { Write-Output "Plan non validé." }
  5       { Write-Output "Fournisseur injoignable : réessayez plus tard." }
  default { Write-Output "Échec (code $LASTEXITCODE)." }
}
```

### sh / bash

```sh
sh /media/vous/CLE/cmd_ai/cmd_ai.sh --no-tui --bypass --prompt "compresse le dossier photos en photos.zip"
code=$?
case "$code" in
  0) echo "Terminé." ;;
  3) echo "Plan non validé." ;;
  5) echo "Fournisseur injoignable : réessayez plus tard." ;;
  *) echo "Échec (code $code)." ;;
esac
```

En TUI fermé, toute la trace (plan, commandes, sorties, réponse du modèle) est écrite sur la sortie standard, et les erreurs sur la sortie d'erreur : vous pouvez la rediriger vers un fichier.
