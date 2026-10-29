---
title: Bugs
description: Comment un bug de cmd_ai est signalé, suivi et corrigé (issue GitHub, fiche, test de non-régression), et l'index des fiches.
---

# Bugs

Chaque bug de cmd_ai a **une issue GitHub** et **une fiche** dans ce dossier, qui suit son état jusqu'à la correction. Un correctif n'est terminé qu'avec son **test de non-régression**, un test qui échoue sans lui.

> **Une faille de sécurité ?** N'ouvrez pas d'issue publique : suivez la [politique de sécurité](https://github.com/brandonviry/cmd_ai/blob/main/SECURITY.md) (avis de sécurité privé). L'issue publique et la fiche sont créées avec le correctif.

## Le processus

1. **L'issue d'abord.** Ouvrez une issue avec le formulaire [« Signaler un bug »](https://github.com/brandonviry/cmd_ai/issues/new?template=bug.yml) : version, OS, shell, étapes de reproduction, comportements attendu et observé, extrait de `log/` (sans la clé API). Son numéro identifie le bug.
2. **La fiche.** Copiez le [modèle de fiche](_modele.md) en `bug/<numéro de l'issue>-<slug>.md`, par exemple `bug/42-exemple-de-slug.md` (slug en minuscules, chiffres et tirets). Remplissez le frontmatter et les quatre sections.
3. **La branche.** Le correctif se développe sur sa propre branche (`bug/<numéro>-<slug>`), jamais directement sur `main`.
4. **Le test d'abord.** Écrivez le test qui reproduit le bug, voyez-le échouer, puis corrigez. Son chemin va dans la clé `test` de la fiche.
5. **Les références.** Ajoutez la fiche à l'index ci-dessous, à la section « Bugs connus » de `CLAUDE.md` et à la page des bugs de la documentation (`doc/`).
6. **L'état.** Tenez `etat` à jour à chaque étape. La PR du correctif passe la fiche à `corrige` et ferme l'issue (`Fixes #<numéro>` dans sa description).

Le job `doc` de la CI vérifie chaque fiche : nom du fichier, clés `etat` et `issue`, valeur de `etat`, test cité et présent dès que le bug est corrigé, références dans `CLAUDE.md`, dans cet index et dans `doc/`.

## La fiche

### Frontmatter

| Clé | Contenu |
| --- | --- |
| `title` | titre de la page sur le site de documentation : `Bug n° <numéro> : <titre>` |
| `description` | une phrase qui résume le bug (site de documentation) |
| `titre` | titre court du bug |
| `etat` | `ouvert`, `en-cours`, `corrige` ou `ferme` (voir ci-dessous) |
| `issue` | URL de l'issue : `https://github.com/brandonviry/cmd_ai/issues/<numéro>` |
| `version` | version de cmd_ai où le bug a été constaté (`cmd_ai --version`) |
| `os` | `windows`, `linux`, `macos` ou `tous` |
| `test` | chemin du test de non-régression, par exemple `tests/oneshot.rs::accents_sous_cmd` ; vide tant que le bug n'est pas corrigé |

### États

| `etat` | Sens |
| --- | --- |
| `ouvert` | signalé, pas encore pris en charge |
| `en-cours` | correctif en cours de développement |
| `corrige` | correctif fusionné sur `main`, test de non-régression en place |
| `ferme` | correctif publié dans une release, ou bug clos sans correctif (la raison est écrite dans la fiche) |

### Sections

- **Symptôme** : ce que voit l'utilisateur, message d'erreur exact compris.
- **Reproduction** : les étapes minimales, avec l'OS, le shell et la version.
- **Cause** : l'origine du bug dans le code (module, fonction, raison).
- **Correctif** : ce qui a changé, le test de non-régression, la PR et la version qui le contient.

## Fiches

| N° | Titre | État | OS | Issue | Test |
| --- | --- | --- | --- | --- | --- |

Aucune fiche pour l'instant.
