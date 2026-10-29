---
title: Bugs
description: Comment les bugs de cmd_ai sont suivis (une issue GitHub et une fiche dans bug/ pour chacun), comment en signaler un, et la liste des bugs connus.
---

## Comment les bugs sont suivis

Chaque bug a **deux traces**, toujours reliées entre elles :

1. une **issue GitHub** sur le dépôt de cmd_ai ;
2. une **fiche** dans le dossier `bug/` du dépôt, nommée `bug/NNN-slug.md` (numéro et nom court), qui donne :
   - son **état** (ouvert, en cours, corrigé…) ;
   - le lien vers l'issue ;
   - une fois corrigé, la version du correctif et le **test de non-régression** qui le couvre : un test automatisé qui échoue sans le correctif.

Toutes les fiches sont recensées dans l'**[index des bugs](../bug/README.md)**. Chaque fiche est aussi citée dans cette page et dans l'index du projet (`CLAUDE.md`).

## Bugs connus

**Aucun bug connu pour l'instant.**

Quand un bug sera identifié, il apparaîtra ici avec un lien vers sa fiche et son état.

## Signaler un bug

Ouvrez une issue sur le dépôt GitHub de cmd_ai (formulaire « Bug »), en indiquant :

- la version : `cmd_ai --version` ;
- l'OS, sa version et l'architecture (par exemple Windows 11 x64, Ubuntu 24.04 aarch64, macOS sur Apple Silicon) ;
- le shell utilisé (`pwsh`, `powershell`, `cmd`, `bash`…), visible dans le bloc de la commande ;
- le mode (`plan` ou `bypass`) et le modèle ;
- ce que vous avez demandé, ce que vous attendiez, et ce qui s'est passé ;
- le code de sortie si c'était un one-shot (voir [Codes de sortie](./codes-de-sortie.md)) ;
- l'extrait utile du journal de la session (dossier `log/`).

Avant de joindre un journal, **relisez-le** : la clé API y est masquée, mais il contient les commandes et leurs sorties, qui peuvent révéler des noms de fichiers ou des contenus privés.

## Une faille de sécurité ?

Ne l'ouvrez **pas** en issue publique : suivez la procédure de `SECURITY.md` (avis de sécurité privé GitHub). Voir [Sécurité](./securite.md).
