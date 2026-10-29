---
name: exemple-projet-npm
description: Travailler dans un projet Node.js géré par npm (installer les dépendances, lancer les scripts, les tests ou le build) quand le dossier courant contient un package.json.
---

# Projet npm : protocole

Ce skill est un exemple : copiez-le dans un nouveau dossier de `skills/`, changez `name` (identique au nom du dossier) et adaptez le protocole à vos projets.

## 1. Reconnaître le projet

1. Vérifier que le dossier courant contient `package.json` ; sinon, le dire à l'utilisateur et s'arrêter.
2. Choisir le gestionnaire d'après le fichier de verrouillage présent :
   - `package-lock.json` (ou aucun) : npm ;
   - `pnpm-lock.yaml` : pnpm ;
   - `yarn.lock` : yarn.
   Utiliser ensuite ce gestionnaire pour toutes les commandes.
3. Lire la section `scripts` de `package.json` pour connaître les scripts disponibles, plutôt que de les deviner.
4. Vérifier que Node.js et le gestionnaire sont installés (`node --version`, `npm --version`). S'ils manquent, expliquer comment les installer, sans rien installer sur la machine sans l'accord de l'utilisateur.

## 2. Installer les dépendances

- Si `node_modules` est absent ou si `package.json` a changé : `npm ci` quand `package-lock.json` existe, sinon `npm install`.
- Ne jamais lancer `npm audit fix --force` ni mettre à jour les dépendances sans que l'utilisateur l'ait demandé.

## 3. Lancer les scripts

- Tests : `npm test` (ou le script `test` du gestionnaire choisi).
- Build : `npm run build` si le script `build` existe.
- Lint : `npm run lint` si le script `lint` existe.
- Serveur de développement (`npm run dev`, `npm start`) : c'est une commande qui ne s'arrête pas ; la lancer seulement si l'utilisateur le demande, en la signalant comme interactive.

## 4. En cas d'échec

1. Lire la sortie d'erreur : souvent un module manquant (relancer l'installation), une version de Node.js trop ancienne (comparer avec le champ `engines` de `package.json`) ou un test en échec (montrer le test et le message).
2. Proposer une seule correction à la fois et expliquer pourquoi.
3. Ne pas supprimer `node_modules` ni le fichier de verrouillage sans l'accord explicite de l'utilisateur.

## 5. Fin de tâche

Résumer en français ce qui a été fait : commandes lancées, résultat des tests ou du build, et ce qu'il reste à faire.
