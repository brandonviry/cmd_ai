---
title: Droits administrateur
description: Comment cmd_ai classe les commandes utilisateur ou administrateur, relance la session en administrateur sous Windows (UAC et --resume), utilise sudo -v puis sudo -n sous Linux et macOS, et réagit à un accès refusé.
---

Certaines commandes demandent des droits administrateur (installer un logiciel, modifier un réglage système, lire un dossier protégé). cmd_ai ne s'élève **jamais de lui-même ni en silence** : l'élévation passe toujours par le consentement de l'OS, y compris en mode bypass.

## Utilisateur ou administrateur

Chaque commande est classée **utilisateur** ou **administrateur** avant de s'exécuter. Le classement combine :

- la déclaration du modèle, qui indique si la commande exige des droits administrateur ;
- des règles fixes de cmd_ai, qui reconnaissent les commandes administrateur courantes (par exemple une commande qui commence par `sudo`).

Le classement est affiché **dans le plan** (badge `admin`) et **à l'exécution**. La barre d'état montre en permanence le niveau de droits de la session : `utilisateur` ou `admin`.

Une commande administrateur dans une session déjà élevée (PowerShell « en tant qu'administrateur », ou `root`) s'exécute directement, après validation du plan en mode plan.

## Sous Windows : relancer la session en administrateur

Une fenêtre UAC ne renvoie pas la sortie de la commande qu'elle lance : cmd_ai ne pourrait ni l'afficher ni la transmettre au modèle. C'est donc **toute la session** cmd_ai qui est relancée en administrateur.

Quand une commande administrateur doit tourner dans une session non élevée :

1. cmd_ai vous explique pourquoi et vous demande si vous voulez relancer en administrateur.
2. Si vous acceptez, il **sauvegarde la tâche** dans le dossier de cmd_ai (`memory/resume.json` : votre demande, le mode, le modèle et le répertoire courant), puis se relance avec la fenêtre UAC de Windows et l'option `--resume`.
3. Vous confirmez dans la fenêtre UAC (avec un mot de passe administrateur si Windows le demande : c'est Windows qui le reçoit, pas cmd_ai).
4. La session non élevée se termine ; la nouvelle session, en administrateur et dans une nouvelle fenêtre, reprend votre demande avec le même mode, le même modèle et le même répertoire courant. Le fichier de reprise est supprimé dès qu'il est lu. En mode plan, vous validez le plan dans cette nouvelle session.

Si vous refusez l'élévation, la commande n'est pas exécutée et le modèle en est informé. Si vous acceptez mais refusez ensuite la fenêtre UAC, rien n'est relancé.

Dans la session élevée, la barre d'état indique `admin` : **toutes** les commandes y tournent avec les droits administrateur. Quittez-la une fois la tâche terminée.

cmd_ai n'utilise pas la commande `sudo` de Windows (désactivée par défaut), ni d'autre outil d'élévation.

## Sous Linux et macOS : `sudo -v`, puis `sudo -n`

Quand une commande administrateur doit tourner :

1. cmd_ai vérifie si `sudo` peut déjà s'exécuter sans mot de passe (`sudo -n true`, par exemple parce que vous venez de vous authentifier).
2. Sinon, il **suspend le TUI** et lance `sudo -v` directement dans votre terminal. Vous tapez votre mot de passe **à `sudo` lui-même** : il ne passe jamais par cmd_ai.
3. Les commandes administrateur s'exécutent ensuite sous la forme `sudo -n …` (non interactif). Si le modèle a écrit `sudo` en tête de la commande, cmd_ai le retire et applique cette même règle.

Si l'authentification échoue, la tâche s'arrête, sans invite bloquée en arrière-plan. Si `sudo -n` échoue en cours de route (par exemple parce que la validité de l'authentification a expiré), la suite s'arrête et cmd_ai vous redemande.

cmd_ai n'utilise jamais `sudo -S` (mot de passe lu sur l'entrée standard).

## Jamais de mot de passe par cmd_ai

Un mot de passe ne doit jamais transiter par cmd_ai : ni dans votre demande, ni dans `config.json`, ni dans la mémoire. Il ne finirait pas seulement dans le contexte du modèle (et donc chez le fournisseur), mais aussi potentiellement dans les journaux. Les seuls endroits où taper un mot de passe sont la fenêtre UAC de Windows et l'invite de `sudo`.

## Quand l'accès est refusé

Une commande classée « utilisateur » peut échouer faute de droits. Quand cmd_ai reconnaît un refus d'accès (« Accès refusé », « Access is denied », `Permission denied`…), l'agent vous explique le problème en français et vous **propose l'élévation**, selon la méthode de votre OS décrite plus haut. Rien n'est relancé avec plus de droits sans votre accord.

## Récapitulatif

| | Windows | Linux et macOS |
| --- | --- | --- |
| Détection des droits | jeton de la session (élevé ou non) | utilisateur `root` ou non |
| Élévation | relance de toute la session via UAC, avec `--resume` | `sudo -v` dans le terminal, puis `sudo -n` |
| Où taper le mot de passe | fenêtre UAC | invite de `sudo` |
| En cas de refus | la commande n'est pas exécutée | échec de `sudo -v` : la tâche s'arrête |
