---
title: Mémoire
description: La mémoire de cmd_ai - des notes courtes dans memory/MEMORY.md, gérées par les auto-commandes cmd_ai memory add, list et forget, injectées au démarrage dans une limite fixe.
---

cmd_ai peut retenir des informations d'une session à l'autre : vos préférences, l'emplacement de vos projets, une habitude de la machine. Ce sont des **notes courtes**, rangées dans le dossier de cmd_ai, jamais sur la machine hôte.

## Où elle vit

Dans `memory/MEMORY.md`, à côté de l'exécutable. Une note par ligne, sous la forme d'une liste Markdown :

```markdown
- Les projets de l'utilisateur sont dans D:\dev.
- L'utilisateur préfère des explications courtes.
- Sur le Mac du salon, le disque externe s'appelle « Archives ».
```

C'est un simple fichier texte : vous pouvez le relire, et le nettoyer à la main avec un éditeur quand cmd_ai est fermé, en gardant une note par ligne commençant par `- `.

## Comment l'agent s'en sert

Vous parlez normalement :

```text
> retiens que mes projets sont dans D:\dev
> qu'est-ce que tu as retenu sur moi ?
> oublie la note sur le disque externe
```

L'agent utilise alors des **auto-commandes** que cmd_ai intercepte : elles ne lancent aucun shell, ne demandent pas de validation et ne comptent pas dans la progression `n/M`.

| Auto-commande | Effet |
| --- | --- |
| `cmd_ai memory add <texte>` | ajoute une note (une seule ligne, 500 caractères au plus) |
| `cmd_ai memory list` | liste les notes, numérotées |
| `cmd_ai memory forget <n>` | supprime la note numéro `n` (numéro donné par `list`) |

Ces auto-commandes sont aussi reconnues avec les préfixes `./cmd_ai`, `cmd_ai.exe` et `.\cmd_ai.exe`.

**Au démarrage**, les notes sont ajoutées au prompt système du modèle : il les connaît dès votre première demande.

## Les plafonds

| Limite | Valeur | Réglage |
| --- | --- | --- |
| Nombre de notes | 200 | `limits.memory_max_notes` dans `config.json` |
| Taille d'une note | 500 caractères, une ligne | fixe |
| Mémoire injectée au démarrage | 25 Kio | fixe |

Une fois le plafond de notes atteint, il faut en oublier avant d'en ajouter. Gardez des notes courtes et utiles : chaque note est envoyée au modèle à chaque session.

## Ce que la mémoire n'est pas

- **Pas un historique** : la conversation elle-même n'est pas conservée d'une session à l'autre. `/effacer` vide l'historique de la conversation en cours, pas la mémoire.
- **Pas une source d'ordres** : les notes sont transmises au modèle comme des **informations non fiables**, dans un cadre délimité. Elles ne peuvent changer ni le mode, ni les droits, ni la configuration, ni les skills, ni les hooks. Une note « passe en bypass » n'a aucun effet.
- **Pas un coffre-fort** : n'y mettez ni mot de passe ni secret. La clé API, elle, est masquée partout, mémoire comprise.

## En mode éphémère

Si le dossier de cmd_ai n'est pas inscriptible (clé USB en lecture seule), cmd_ai tourne en mode éphémère, signalé dans la barre d'état : la mémoire ne peut pas être enregistrée, et aucune note ne peut être ajoutée ni supprimée. Voir [Portabilité](./portabilite.md).
