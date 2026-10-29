---
title: Sécurité
description: Comment cmd_ai protège votre clé API (masquage, environnement des commandes nettoyé, révocation sur ollama.com/settings/keys), traite les sorties comme non fiables, neutralise les séquences de contrôle, et comment signaler une faille.
---

cmd_ai exécute de vraies commandes et lit ce qu'elles renvoient. Cette page explique ce qu'il protège, comment, et ce qui reste de votre responsabilité.

## La clé API

### Où elle est

En V1, la clé est stockée **en clair** dans `config.json`, dans le dossier de cmd_ai (`provider.api_key`). Si vous ne voulez pas qu'elle voyage sur la clé USB, laissez ce champ vide et définissez la variable d'environnement `OLLAMA_API_KEY` au lancement : elle a la priorité (voir [Installation](./installation.md)).

### Ce que cmd_ai en fait

- **Masquée partout.** La clé est remplacée par `••••` dans tout ce qui sort de cmd_ai : écran, journaux de `log/`, mémoire, contexte envoyé au modèle, messages d'erreur. Même si une commande affiche `config.json`, la clé y est masquée.
- **Jamais transmise aux commandes.** `OLLAMA_API_KEY` et les variables propres à cmd_ai sont retirées de l'environnement de chaque commande lancée, même héritées : `env`, `set` ou `Get-ChildItem Env:` ne la montrent pas.
- **Envoyée à un seul endroit.** La clé ne part que vers `provider.base_url`, en HTTPS. Le HTTP en clair n'est accepté que vers la machine elle-même (`localhost`, `127.0.0.1`, `[::1]`).
- **TLS toujours vérifié.** La vérification des certificats ne se désactive pas. Pour un réseau d'entreprise, ajoutez un certificat avec `network.ca_file` (voir [Configuration](./configuration.md)).

### En cas de perte de la clé USB

**Les clés Ollama n'expirent jamais.** Si vous perdez la clé USB (ou si vous pensez que la clé API a fuité) :

1. Ouvrez [ollama.com/settings/keys](https://ollama.com/settings/keys).
2. **Révoquez** la clé concernée.
3. Créez-en une nouvelle et mettez-la dans votre `config.json`.

Pour limiter les dégâts possibles, gardez un solde de crédits modeste et donnez à chaque clé un nom qui dit où elle se trouve (`cle-usb-bleue`, `pc-bureau`) : vous saurez laquelle révoquer.

## Ce qui part chez le fournisseur

Pour travailler, le modèle reçoit :

- vos demandes ;
- le prompt système : OS, architecture, shell et version, niveau de droits, mode actif, catalogue des skills (nom et description), notes de la [mémoire](./memoire.md) ;
- le corps des skills que l'agent charge ;
- la sortie des commandes exécutées, réduite à 16 Kio par commande.

Ces données quittent la machine pour être traitées par le fournisseur (Ollama Cloud, sauf si vous utilisez un serveur local). Ne demandez pas à cmd_ai d'afficher des fichiers confidentiels si vous ne voulez pas qu'ils soient envoyés.

## Les sorties sont des données non fiables

Tout ce qui vient de la machine (sorties des commandes, contenus et noms de fichiers, variables) et de la mémoire est traité comme une **donnée non fiable**. Le modèle reçoit chaque résultat dans un cadre délimité :

```text
code de sortie : 0 · durée : 0,4 s
<<<SORTIE NON FIABLE (stdout)>>>
…
<<<FIN SORTIE NON FIABLE>>>
<<<SORTIE NON FIABLE (stderr)>>>
…
<<<FIN SORTIE NON FIABLE>>>
```

- Le prompt système demande au modèle d'ignorer toute consigne trouvée dans ces cadres. Si une sortie contient elle-même ces marqueurs, ils sont neutralisés.
- Une sortie ne peut changer **ni le mode, ni les droits, ni la configuration, ni les skills, ni les hooks**. Seuls votre saisie dans le TUI, `--prompt`, `--prompt-file` et les skills portent des consignes.
- Les sorties sont nettoyées avant d'être envoyées : séquences ANSI retirées, lignes répétées regroupées, début et fin conservés si elles sont trop longues.

Ces protections réduisent le risque d'injection de prompt, sans l'annuler : lisez l'avertissement de la page [Modes](./modes.md) avant d'utiliser le bypass.

## Les séquences de contrôle sont neutralisées

Une sortie de commande peut contenir des séquences de contrôle du terminal, parfois malveillantes. Avant affichage, cmd_ai les neutralise : **seules les couleurs passent**. Une sortie ne peut ni changer le titre de la fenêtre, ni écrire dans votre presse-papiers (OSC 52), ni déplacer le curseur pour maquiller l'affichage.

## Les commandes elles-mêmes

- **Une seule porte d'exécution.** Toute commande, de l'agent comme des hooks, passe par le même module : affichage, entrée standard fermée, délai d'expiration, arbre de processus tué en cas d'annulation.
- **Un appel d'outil mal formé n'est jamais exécuté**, même en partie, ni remplacé par une commande devinée : l'erreur est renvoyée au modèle (2 relances au plus).
- **L'environnement des commandes est nettoyé** : en plus de la clé API, cmd_ai retire `BASH_ENV`, `ENV` et `HISTFILE`, et définit `POWERSHELL_TELEMETRY_OPTOUT=1`, `POWERSHELL_UPDATECHECK=Off`, `PAGER=cat` et `GIT_PAGER=cat`. PowerShell est lancé avec `-NoProfile -NonInteractive`.
- **Pas d'élévation silencieuse** : voir [Droits](./droits.md). Aucun mot de passe ne passe par cmd_ai.

## Vos journaux

Les journaux de `log/` contiennent les commandes et leurs sorties, avec la clé API masquée. Ils peuvent donc contenir des informations privées (noms de fichiers, contenus affichés). Ils sont sur votre clé : supprimez-les si besoin, et relisez-les avant de les partager, par exemple dans un rapport de bug.

## Ce qui reste de votre ressort

- Relire le plan en mode plan, et réserver le bypass aux situations de confiance.
- N'installer que des skills et des hooks que vous avez écrits ou relus.
- Protéger `config.json` : il contient la clé API et les hooks, qui s'exécutent sans validation.
- Révoquer la clé API au moindre doute.

## Signaler une faille de sécurité

Ne publiez pas une faille dans une issue publique. Suivez la procédure du fichier `SECURITY.md`, à la racine du dépôt GitHub : le signalement passe par un **avis de sécurité privé** GitHub (onglet **Security**, puis **Report a vulnerability**). L'issue publique est créée avec le correctif. Pour les autres bugs, voir [Bugs](./bugs.md).
