---
title: Installation
description: Télécharger le paquet portable de cmd_ai, le copier sur une clé USB exFAT, créer sa clé API Ollama Cloud et réussir le premier lancement sous Windows, Linux et macOS.
---

cmd_ai n'a pas d'installateur : c'est un dossier qui contient les exécutables et leurs fichiers. Vous le décompressez où vous voulez (sur votre disque ou sur une clé USB), et c'est ce dossier que cmd_ai utilise, rien d'autre.

## Prérequis

- **Windows** 10 (version 1809 ou plus récente) ou Windows 11, en x64.
- **Linux** x86_64 ou aarch64 (ARM 64 bits). Les binaires sont statiques : aucune bibliothèque à installer.
- **macOS** sur Intel ou Apple Silicon (un seul binaire universel).
- Un terminal qui comprend les séquences VT : c'est le cas de Windows Terminal, de la console de Windows 10 1809 et plus, et de tous les terminaux Unix modernes.
- Une **clé API Ollama Cloud** (voir plus bas), ou un serveur Ollama local.

## 1. Télécharger le paquet portable

1. Ouvrez la page **Releases** du dépôt GitHub de cmd_ai.
2. Téléchargez `cmd_ai-vX.Y.Z-portable.zip` (X.Y.Z = numéro de version) et le fichier des sommes SHA-256 publié avec lui.
3. Vérifiez l'empreinte du zip (facultatif mais conseillé) et comparez-la à celle publiée :

```powershell
# Windows (PowerShell)
Get-FileHash .\cmd_ai-vX.Y.Z-portable.zip -Algorithm SHA256
```

```sh
# Linux
sha256sum cmd_ai-vX.Y.Z-portable.zip
# macOS
shasum -a 256 cmd_ai-vX.Y.Z-portable.zip
```

4. Décompressez le zip à l'endroit voulu.

Les releases sont construites et publiées uniquement par la CI GitHub, jamais depuis un poste local.

## 2. Le contenu du dossier

Un même dossier sert les trois OS : tous les binaires partagent la même configuration, les mêmes journaux, la même mémoire et les mêmes skills.

```text
cmd_ai/
├── cmd_ai.exe             # Windows x64
├── cmd_ai-linux-x86_64    # Linux x86_64 (binaire statique)
├── cmd_ai-linux-aarch64   # Linux ARM 64 bits (binaire statique)
├── cmd_ai-macos           # macOS (binaire universel Intel + Apple Silicon)
├── cmd_ai.sh              # lanceur Linux/macOS : choisit le bon binaire
├── cmd_ai.command         # lanceur macOS par double-clic
├── config.example.json    # modèle de configuration
├── config.json            # votre configuration (créée au premier lancement)
├── log/                   # journaux de session
├── memory/                # mémoire de l'agent (MEMORY.md)
└── skills/                # playbooks rédigés à la main (SKILL.md)
```

`config.json` n'existe pas dans le zip : il est créé au premier lancement (voir l'étape 5), ou par vous à partir de `config.example.json`.

## 3. Sur une clé USB : formatez-la en exFAT

Pour emporter cmd_ai sur une clé USB qui marche sur les trois OS, **formatez-la en exFAT** :

- **FAT32** : sous Linux, la clé est en général montée avec l'option `showexec`, qui interdit d'exécuter un fichier dont le nom ne finit pas par `.exe`, `.com` ou `.bat`. Le binaire Linux ne peut donc pas tourner (voir [Portabilité](./portabilite.md)).
- **NTFS** : macOS ne sait pas y écrire sans logiciel supplémentaire. cmd_ai y tournerait en mode éphémère (ni journaux ni mémoire).
- **exFAT** : lu et écrit par Windows, Linux et macOS, sans blocage de l'exécution.

Formater **efface tout le contenu** de la clé. Pour la formater :

- Windows : Explorateur de fichiers, clic droit sur la clé, **Formater…**, système de fichiers **exFAT**.
- macOS : **Utilitaire de disque**, sélectionnez la clé, **Effacer**, format **ExFAT**.
- Linux : l'application **Disques** (GNOME Disks), ou l'outil de formatage de votre bureau, en choisissant exFAT.

Copiez ensuite le dossier `cmd_ai/` décompressé sur la clé.

## 4. Créer votre clé API Ollama Cloud

1. Créez un compte sur [ollama.com](https://ollama.com) (ou connectez-vous).
2. Ouvrez la page des clés : [ollama.com/settings/keys](https://ollama.com/settings/keys).
3. Créez une nouvelle clé, donnez-lui un nom parlant (par exemple `cle-usb-cmd_ai`), puis copiez-la.

À savoir :

- **Les clés Ollama n'expirent jamais.** Si vous perdez la clé USB qui la contient, révoquez-la tout de suite sur la même page (voir [Sécurité](./securite.md)).
- Gardez un solde de crédits modeste : une clé perdue peut être utilisée par quelqu'un d'autre.
- Offre au 28/10/2026 (source : ollama.com/pricing, à revérifier car les tarifs ont déjà changé) : le plan gratuit « Pay as you go » autorise 3 requêtes simultanées et facture tous les modèles au token, sauf **Beam**, gratuit pour une durée limitée. cmd_ai ne garde qu'une requête en cours à la fois.

## 5. Premier lancement

### Windows

- Double-cliquez sur `cmd_ai.exe` : une fenêtre de terminal s'ouvre avec le TUI.
- Ou, dans Windows Terminal ou PowerShell, placez-vous dans le dossier où vous voulez travailler et lancez `D:\cmd_ai\cmd_ai.exe` (en remplaçant `D:` par la lettre de votre clé). Les commandes de l'agent s'exécutent dans le répertoire courant du terminal.

Au premier lancement d'un fichier téléchargé, SmartScreen peut afficher « Windows a protégé votre ordinateur » : cliquez sur **Informations complémentaires**, puis **Exécuter quand même**. Si Smart App Control, AppLocker ou une stratégie d'entreprise bloque le binaire, voir [Portabilité](./portabilite.md) : la V1 n'est pas signée.

### Linux

Dans un terminal :

```sh
sh /media/vous/CLE/cmd_ai/cmd_ai.sh
```

`cmd_ai.sh` choisit le binaire adapté à votre processeur (`cmd_ai-linux-x86_64` ou `cmd_ai-linux-aarch64`) et lui transmet vos options. Si le système répond « Permission non accordée » alors que la clé est en exFAT, rendez les binaires exécutables : `chmod +x cmd_ai-linux-*` dans le dossier de cmd_ai.

### macOS

- Double-cliquez sur `cmd_ai.command` : le Terminal s'ouvre et lance cmd_ai.
- Ou, dans le Terminal : `sh /Volumes/CLE/cmd_ai/cmd_ai.sh`.

Au premier lancement :

- macOS peut demander « Terminal souhaite accéder aux fichiers d'un volume amovible » : répondez **Autoriser**.
- Gatekeeper peut refuser d'ouvrir `cmd_ai-macos` (« le développeur ne peut pas être vérifié ») : ouvrez **Réglages Système → Confidentialité et sécurité** et cliquez sur **Ouvrir quand même**, ou retirez l'attribut de quarantaine (détails dans [Portabilité](./portabilite.md)).

### La configuration du premier lancement

Si `config.json` n'existe pas encore et que vous êtes dans un terminal interactif, cmd_ai vous propose de le créer : il copie `config.example.json` en `config.json`, vous demande votre clé API Ollama et l'enregistre dans ce fichier, à côté de l'exécutable. Rien n'est écrit ailleurs.

Si vous refusez, ou si cmd_ai n'est pas lancé depuis un terminal interactif, il affiche comment faire à la main et s'arrête avec le [code de sortie](./codes-de-sortie.md) 4 :

1. Copiez `config.example.json` en `config.json`, dans le même dossier.
2. Ouvrez `config.json` avec un éditeur de texte et collez votre clé dans `provider.api_key` :

```json
{
  "version": 1,
  "provider": {
    "base_url": "https://ollama.com/v1",
    "api_key": "collez-votre-clé-ici",
    "model": "beam",
    "roles": { "fast": "gpt-oss:120b", "fr": "mistral-large-4" }
  }
}
```

cmd_ai lit toujours le `config.json` situé **à côté de l'exécutable**, jamais celui du répertoire courant. Il ne réécrit jamais un `config.json` existant. Toutes les autres clés sont décrites dans [Configuration](./configuration.md).

### Vérifier que tout marche

```text
cmd_ai --version
cmd_ai --prompt "affiche la date et l'heure"
```

La première commande affiche `cmd_ai X.Y.Z`. La seconde présente un plan d'une commande : validez avec `o`, la date s'affiche avec la progression `1/1`, puis la réponse du modèle. Un fichier de session apparaît dans `log/`.

## Ne pas laisser la clé API sur la clé USB : `OLLAMA_API_KEY`

Si la variable d'environnement `OLLAMA_API_KEY` est définie et non vide, elle **remplace** la clé de `config.json`. Vous pouvez alors laisser `"api_key": ""` dans le fichier :

```powershell
# Windows (PowerShell), pour la session en cours seulement
$env:OLLAMA_API_KEY = "votre-clé"
.\cmd_ai.exe
```

```sh
# Linux / macOS, pour la session en cours seulement
export OLLAMA_API_KEY="votre-clé"
sh cmd_ai.sh
```

La variable ne vit que dans ce terminal et n'est jamais transmise aux commandes lancées par cmd_ai. Attention : sur une machine qui n'est pas la vôtre, la ligne que vous tapez peut rester dans l'historique du shell de l'hôte (PSReadLine, `.bash_history`…). Cette méthode convient surtout à votre propre machine.

## Utiliser un serveur Ollama local

Si Ollama est installé sur la machine, cmd_ai peut l'utiliser à la place du cloud, sans clé API :

```json
{
  "version": 1,
  "provider": {
    "base_url": "http://localhost:11434/v1",
    "api_key": "",
    "model": "gpt-oss:20b"
  }
}
```

- `model` doit être un modèle déjà téléchargé sur ce serveur, et qui sait appeler des outils (tool calling) : cmd_ai en a besoin pour proposer des commandes.
- Le HTTP en clair n'est accepté que vers la machine elle-même (`localhost`, `127.0.0.1`, `[::1]`). Un serveur Ollama sur une autre machine du réseau doit être joint en HTTPS.
- Installer Ollama écrit évidemment sur la machine : ce mode convient à votre propre poste, pas au scénario « clé USB chez quelqu'un ».

## Mettre à jour cmd_ai

Téléchargez le nouveau zip et remplacez les binaires, les lanceurs et `config.example.json`. Gardez votre `config.json`, `memory/`, `skills/` et `log/` : cmd_ai ne les écrase jamais. Comparez votre `config.json` avec le nouveau `config.example.json` pour découvrir les nouvelles options.
