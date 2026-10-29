---
title: FAQ
description: Questions fréquentes sur cmd_ai - coût, confidentialité, sécurité, hors ligne, modèles, shells, clé USB, scripts et limites de la V1.
---

## Les bases

### Faut-il connaître les commandes pour utiliser cmd_ai ?

Non, c'est tout l'intérêt. Vous décrivez ce que vous voulez en français ; cmd_ai écrit les commandes pour l'OS et le shell de la machine. En mode plan, vous les voyez avant qu'elles tournent, ce qui permet aussi d'apprendre.

### cmd_ai est-il gratuit ?

cmd_ai lui-même est libre et gratuit (licence MIT). Le modèle, lui, est fourni par Ollama Cloud : au 28/10/2026, Beam (le modèle par défaut) y est gratuit pour une durée limitée, et les autres modèles sont facturés au token. Vérifiez les tarifs sur ollama.com/pricing. Avec un serveur Ollama local, il n'y a rien à payer.

### Sur quels systèmes fonctionne-t-il ?

Windows 10 (1809 et plus) et 11 en x64, Linux x86_64 et aarch64, macOS Intel et Apple Silicon. Un même dossier les sert tous (voir [Portabilité](./portabilite.md)).

### Peut-il faire autre chose qu'exécuter des commandes ?

Il peut aussi répondre sans commande : traduire, expliquer un message d'erreur, résumer une sortie. Mais son seul outil est l'exécution de commandes : pas d'accès direct aux fichiers ni au web, tout passe par des commandes visibles.

## Sécurité et confidentialité

### L'IA peut-elle faire quelque chose sans mon accord ?

En **mode plan** (le défaut), non : rien ne s'exécute avant votre validation, et toute commande hors du plan validé vous est redemandée. En **mode bypass**, que vous seul pouvez activer, les commandes s'exécutent sans confirmation, sauf celles de la liste des garde-fous. Voir [Modes](./modes.md).

### Et si un fichier contient des consignes pour piéger l'agent ?

C'est l'injection de prompt. Les sorties sont transmises au modèle comme des données non fiables et ne peuvent changer ni le mode ni les droits. Mais en bypass, une commande inspirée par un contenu piégé peut s'exécuter : restez en mode plan sur des contenus inconnus. Voir [Modes](./modes.md).

### Qu'est-ce qui est envoyé au fournisseur ?

Vos demandes, le contexte de la machine (OS, shell, droits), vos notes de mémoire, les skills chargés et la sortie des commandes (16 Kio au plus par commande). La clé API ne sert qu'à vous authentifier auprès du fournisseur : elle est masquée dans tout ce contenu. Voir [Sécurité](./securite.md).

### J'ai perdu ma clé USB. Que faire ?

Révoquez tout de suite la clé API sur [ollama.com/settings/keys](https://ollama.com/settings/keys) : les clés Ollama n'expirent jamais. Créez-en une nouvelle pour votre prochaine clé USB.

### cmd_ai laisse-t-il des traces sur la machine ?

cmd_ai n'écrit rien hors de son dossier. Mais l'OS garde ses propres traces (Prefetch, Amcache, historique des périphériques USB…), et les programmes que vous faites lancer gardent les leurs (historique du navigateur, caches). Le détail est dans [Portabilité](./portabilite.md).

### Puis-je lui donner mon mot de passe pour une commande administrateur ?

Non, et ce n'est jamais nécessaire : sous Windows, c'est la fenêtre UAC qui vous le demande ; sous Linux et macOS, c'est `sudo` lui-même, directement dans le terminal. Voir [Droits](./droits.md).

## Modèles et langue

### Pourquoi le modèle me répond-il parfois mal en français ?

Le prompt système exige des réponses en français, mais Beam a été développé surtout pour l'anglais. Pour un meilleur français, utilisez le rôle `fr` : `--model fr` au lancement, ou `/model fr` dans le TUI.

### Comment changer de modèle ?

Pour une session : `--model fast` ou `/model fast`. Pour toujours : changez `provider.model` dans `config.json`. Voir [Configuration](./configuration.md). Faites un essai réel après un changement : la capacité à appeler l'outil varie d'un modèle à l'autre.

### J'ai l'erreur « rôle inconnu ».

Le nom passé à `--model` n'est pas un rôle de `provider.roles` et ne ressemble pas à un nom de modèle (sans `:`, `/` ni `.`). Le message liste les rôles disponibles. Ajoutez un rôle dans `config.json`, ou donnez le nom complet du modèle.

### Puis-je utiliser cmd_ai hors ligne ?

Avec un serveur Ollama local (`"base_url": "http://localhost:11434/v1"`), oui. Sinon, sans réseau, cmd_ai affiche un message d'erreur et n'exécute rien (code 5).

### Puis-je utiliser OpenAI, Anthropic ou un autre fournisseur ?

Pas en V1 : cmd_ai est conçu et testé pour Ollama uniquement (cloud ou local). D'autres fournisseurs pourront venir plus tard.

### Le modèle met du temps à répondre, ou répond « file d'attente ».

Le fournisseur limite le nombre de requêtes simultanées. cmd_ai n'en garde qu'une en cours et réessaie automatiquement (au plus 3 fois) en cas de saturation passagère ou d'erreur du serveur. Si les crédits sont épuisés (402) ou le quota atteint, il s'arrête avec un message clair.

## Au quotidien

### Dans quel dossier les commandes s'exécutent-elles ?

Dans le répertoire courant du terminal d'où vous avez lancé cmd_ai. Placez-vous dans votre dossier de travail avant de le lancer, ou dites-le simplement dans la demande (« dans le dossier Documents, … »).

### Pourquoi PowerShell et pas cmd ?

Sous Windows, cmd_ai utilise PowerShell 7 s'il est présent, sinon Windows PowerShell 5.1 ; `cmd` seulement si c'est demandé. Vous pouvez le demander : « fais-le en cmd ». Sous Linux et macOS, c'est `bash` (ou `sh`).

### Une commande semble bloquée.

Les commandes ne reçoivent pas de saisie clavier : une commande qui attend une réponse est arrêtée au bout du délai (120 s par défaut). Appuyez sur Échap pour l'arrêter tout de suite. Quand une commande a vraiment besoin de votre clavier, l'agent peut demander une étape interactive qui vous rend le terminal.

### Comment l'utiliser dans un script ?

`cmd_ai --no-tui --bypass --prompt "…"`, puis testez le code de sortie (voir [Codes de sortie](./codes-de-sortie.md)). Sans `--bypass`, un script ne peut pas valider le plan : cmd_ai sort avec le code 3 sans rien exécuter.

### Les couleurs sont illisibles dans mon terminal.

Essayez `"ui": { "theme": "light" }` sur fond clair, ou `"theme": "dark"` sur fond sombre. `--color none` (ou la variable `NO_COLOR`) retire toutes les couleurs : le TUI reste lisible, car chaque information porte aussi un libellé.

### Où sont les journaux ?

Dans le dossier `log/` de cmd_ai, un fichier par session (`AAAA-MM-JJ_HHMMSS.log`), horodaté en UTC, avec la clé API masquée. Le dossier est plafonné à 50 Mio : les plus anciens fichiers sont supprimés.

### L'agent a oublié ce que je lui ai dit hier.

La conversation n'est pas gardée d'une session à l'autre. Pour qu'il retienne une information, dites-le : « retiens que… ». Voir [Mémoire](./memoire.md).

### Comment mettre à jour cmd_ai ?

Remplacez les binaires, les lanceurs et `config.example.json` par ceux du nouveau zip, et gardez `config.json`, `memory/`, `skills/` et `log/`. Voir [Installation](./installation.md).

## Clé USB et blocages

### Quel format pour la clé USB ?

**exFAT**. En FAT32, Linux refuse d'exécuter le binaire ; en NTFS, macOS ne peut pas écrire. Voir [Installation](./installation.md).

### Windows ou macOS refusent de lancer cmd_ai.

La V1 n'est pas signée. SmartScreen et Gatekeeper se contournent en quelques clics ; Smart App Control, AppLocker et les interdictions d'exécution depuis une clé USB, non. Tout est détaillé dans [Portabilité](./portabilite.md). La signature est prévue avant la 1.0.

### Ma clé est en lecture seule.

cmd_ai fonctionne en mode éphémère : ni journaux ni mémoire, avec la mention « éphémère » dans la barre d'état. Rien n'est écrit ailleurs.
