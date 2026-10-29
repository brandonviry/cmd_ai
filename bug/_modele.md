---
title: "Bug n° NNN : titre court"
description: "Une phrase qui résume le bug."
titre: "Titre court du bug"
etat: ouvert
issue: https://github.com/brandonviry/cmd_ai/issues/NNN
version: "0.1.0"
os: tous
test: ""
---

Modèle de fiche : copiez ce fichier en `bug/<numéro de l'issue>-<slug>.md`, remplacez `NNN` et chaque texte d'exemple, puis supprimez ce paragraphe. Le processus complet est décrit dans l'[index des bugs](README.md).

## Symptôme

Ce que voit l'utilisateur : message d'erreur exact, sortie, comportement inattendu.

## Reproduction

1. Première étape, par exemple : lancer `cmd_ai --no-tui --prompt "affiche la date"` sous Windows PowerShell 5.1.
2. Étape suivante.

- **Attendu** : ce qui devrait se passer.
- **Observé** : ce qui se passe.
- **Environnement** : OS et version, shell, terminal, version de cmd_ai.

## Cause

L'origine du bug dans le code : module, fonction, raison. « À déterminer » tant qu'elle n'est pas connue.

## Correctif

Ce qui a changé, et pourquoi.

- **Test de non-régression** : `tests/fichier.rs::nom_du_test` (il échoue sans le correctif).
- **PR** : lien vers la pull request.
- **Version** : première version qui contient le correctif.
