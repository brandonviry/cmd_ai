---
title: Skills
description: Écrire des skills, des playbooks rédigés à la main que l'agent lit et suit - format SKILL.md, dossier skills/, règles du frontmatter, chargement par cmd_ai skill, et un exemple complet.
---

Un **skill** est un playbook que **vous** rédigez : il décrit à l'agent comment s'y prendre pour un type de tâche (« dans un projet npm, voilà les commandes à utiliser »). L'agent le lit et le suit ; sa tâche reste d'exécuter des commandes.

Les skills sont la version « pilotée par le LLM » des extensions de cmd_ai. Pour des déclencheurs sûrs et déterministes, voyez les [hooks](./hooks.md).

## Où les ranger

Chaque skill est un dossier dans `skills/`, qui contient un fichier `SKILL.md` :

```text
cmd_ai/
└── skills/
    ├── projet-npm/
    │   └── SKILL.md
    └── sauvegarde-photos/
        └── SKILL.md
```

Le paquet portable contient un exemple : `skills/exemple-projet-npm/SKILL.md`.

Le format est celui des **Agent Skills** (`SKILL.md`), le même que Claude Code : un skill écrit pour l'un sert à l'autre.

## Le format de `SKILL.md`

Un fichier texte UTF-8 en deux parties :

1. un **frontmatter** YAML, tout en haut, entre deux lignes `---` ;
2. le **corps** en Markdown : les consignes elles-mêmes.

```markdown
---
name: projet-npm
description: Travailler dans un projet Node.js géré avec npm (dossier qui contient package.json).
---

Les consignes pour l'agent…
```

### Les règles du frontmatter

| Champ | Obligatoire | Règle |
| --- | --- | --- |
| `name` | oui | identique au **nom du dossier** ; 64 caractères au plus ; lettres minuscules sans accent, chiffres et tirets (`a-z`, `0-9`, `-`) ; ni tiret au début ou à la fin, ni double tiret |
| `description` | oui | 1 024 caractères au plus ; dit **quand** utiliser le skill |

Exemples de noms : `projet-npm`, `sauvegarde-photos`, `serveur-web2` sont valides ; `Projet_NPM`, `-npm`, `projet--npm` ou `../config` ne le sont pas.

Un skill invalide (frontmatter absent, nom qui ne correspond pas au dossier, champ trop long) n'est pas chargé, et cmd_ai le signale par un avertissement.

### Bien écrire la description

C'est **la seule partie que le modèle voit d'emblée** : c'est sur elle qu'il décide d'ouvrir le skill ou non. Écrivez-la comme une réponse à « dans quelle situation ce skill sert-il ? », avec les mots que vous utiliseriez dans une demande.

## Comment l'agent les utilise

1. **Au démarrage**, cmd_ai lit le catalogue : le nom et la description de chaque skill (pas le corps) figurent dans le prompt système du modèle.
2. **Quand une demande correspond**, l'agent charge le corps avec l'auto-commande `cmd_ai skill projet-npm`. cmd_ai l'intercepte : **aucun shell n'est lancé**, il renvoie simplement le contenu du `SKILL.md` au modèle.
3. L'agent suit alors le protocole, en proposant ses commandes comme d'habitude.

L'agent peut aussi demander la liste avec `cmd_ai skill list`. Ces auto-commandes ne demandent pas de validation et ne comptent pas dans la progression `n/M`.

Le nom demandé est toujours vérifié : `cmd_ai skill ../config` est refusé sans lire aucun fichier. Seul le dossier `skills/` est lu.

## Ce qu'un skill ne peut pas faire

- **Accorder des permissions** : un skill guide l'agent, il ne change ni le mode, ni les droits, ni les garde-fous. Les commandes qu'il inspire passent par le plan et la validation comme les autres.
- **S'écrire tout seul** : cmd_ai ne crée et ne modifie jamais de skill de sa propre initiative. C'est vous qui les rédigez.

À l'inverse des sorties de commandes, un skill **porte des consignes** que l'agent suit. N'installez que des skills que vous avez écrits ou relus, comme vous le feriez pour un script.

## Un exemple complet

`skills/projet-npm/SKILL.md` :

```markdown
---
name: projet-npm
description: Travailler dans un projet Node.js géré avec npm (dossier qui contient package.json) - installer les dépendances, lancer les scripts et les tests, vérifier les mises à jour.
---

# Projet npm

Applique ces règles quand le répertoire courant contient `package.json`.

## Avant tout

1. Lis `package.json` pour connaître les scripts disponibles
   (`npm run` sans argument les liste).
2. Vérifie que `node` et `npm` sont installés (`node --version`, `npm --version`).
   S'ils manquent, dis-le à l'utilisateur au lieu de tenter une installation.

## Installer les dépendances

- Si `package-lock.json` existe, utilise `npm ci`.
- Sinon, utilise `npm install`.

## Lancer

- Les tests : `npm test`.
- Un script : `npm run <nom>`, avec un nom lu dans `package.json`.
- N'invente jamais un nom de script.

## Mises à jour

- Commence toujours par `npm outdated` et montre le résultat.
- Ne mets à jour que ce que l'utilisateur a demandé.

## Interdits

- Ne lance jamais `npm publish` ni `npm unpublish`, sauf demande explicite.
- Ne supprime pas `node_modules` sans le dire dans le plan.
```

Avec ce skill, la demande « installe et lance les tests de ce projet » amène l'agent à charger `projet-npm`, puis à vous proposer un plan du type `npm ci` puis `npm test`.

## Conseils

- **Un skill, un sujet.** Plusieurs petits skills valent mieux qu'un gros.
- **Pensez multi-OS.** Le dossier sert Windows, Linux et macOS : si une étape diffère d'un OS à l'autre, écrivez les deux variantes.
- **Restez concret** : des commandes exactes et des règles courtes se suivent mieux que de longues explications.
- **Écrivez en français** si vous voulez : le modèle répond de toute façon en français.
