---
title: Portabilité et limites V1
description: Un seul dossier pour Windows, Linux et macOS, rien d'écrit sur la machine hôte, et les limites honnêtes - traces de l'OS, clé en lecture seule, binaires non signés (SmartScreen, Smart App Control, AppLocker, Gatekeeper) et FAT32 sous Linux.
---

cmd_ai est fait pour voyager : un dossier, sur une clé USB ou dans un zip, que l'on branche sur n'importe quelle machine. Cette page dit ce qui est garanti, et ce qui ne l'est pas.

## Un seul dossier pour trois OS

Le même dossier contient les binaires de chaque OS, et ils partagent tout le reste :

```text
cmd_ai/
├── cmd_ai.exe             # Windows x64
├── cmd_ai-linux-x86_64    # Linux x86_64
├── cmd_ai-linux-aarch64   # Linux ARM 64 bits
├── cmd_ai-macos           # macOS universel (Intel + Apple Silicon)
├── cmd_ai.sh              # lanceur Linux/macOS
├── cmd_ai.command         # lanceur macOS par double-clic
├── config.json            # une seule configuration
├── log/                   # les journaux de toutes les sessions
├── memory/                # une seule mémoire
└── skills/                # les mêmes skills
```

Les fichiers sont en UTF-8 et les chemins de la configuration sont relatifs au dossier : la clé fonctionne quelle que soit la lettre de lecteur ou le point de montage.

cmd_ai trouve son dossier **à partir de l'emplacement de l'exécutable**, jamais à partir du répertoire courant : vous pouvez le lancer depuis n'importe quel dossier de travail. Pour utiliser un autre dossier, passez `--home <dossier>` ou définissez `CMD_AI_HOME`.

## La règle d'or : rien d'écrit sur l'hôte

cmd_ai lui-même n'écrit **rien hors de son dossier** :

- ni dans le profil utilisateur, ni dans `%APPDATA%`, `~/.config` ou `~/Library` ;
- ni dans le dossier temporaire du système ;
- ni dans le registre Windows, ni dans un trousseau ;
- et il ne modifie pas de lui-même la configuration de la machine (`PATH`, politique d'exécution PowerShell, variables persistantes).

Les shells qu'il lance tournent dans un environnement qui limite les traces : PowerShell sans profil, sans télémétrie ni vérification de mise à jour, et avec son cache d'analyse de modules détourné du profil ; pas d'historique de commandes (`HISTFILE` vidé) ; pas de fichier de démarrage injecté (`BASH_ENV` et `ENV` retirés) ; `PAGER=cat` pour ne jamais rester bloqué dans un pager.

## Ce que cmd_ai ne peut pas empêcher

Être honnête fait partie de la règle : cmd_ai **ne promet pas « zéro trace »**.

- **Les traces de l'OS.** Windows, macOS et Linux enregistrent eux-mêmes certaines exécutions et certains branchements : sous Windows, Prefetch, Amcache, SRUM, historique des périphériques USB ; sous macOS, l'autorisation d'accès au volume amovible accordée au Terminal (conservée sur la machine) ; et partout, les journaux du système. cmd_ai n'y a pas accès et ne peut pas les éviter.
- **Le cache de démarrage de PowerShell.** À chaque lancement, PowerShell 7 (`pwsh`) et Windows PowerShell 5.1 écrivent un petit fichier d'optimisation du démarrage, `StartupProfileData-NonInteractive`, dans `%LOCALAPPDATA%\Microsoft\PowerShell\` (ou `…\Microsoft\Windows\PowerShell\` pour la 5.1 ; `~/.cache/powershell/` sous Linux et macOS). Il ne contient que des statistiques de compilation de .NET, aucune donnée ni commande, et il est réécrit à chaque fois. PowerShell n'offre aucune option pour le désactiver : c'est la seule exception connue, vérifiée par le test `tests/portability.rs`.
- **Les commandes que vous demandez.** « Crée un fichier sur le bureau » écrit sur le bureau : c'est votre demande, une commande comme une autre, affichée et validée.
- **Les programmes lancés par les commandes.** Un navigateur ouvert pour YouTube garde son historique ; `npm`, `pip` ou `git` peuvent écrire dans leurs caches et leurs fichiers de configuration. Ces écritures appartiennent à ces programmes.
- **Le shell d'où vous lancez cmd_ai.** Si vous tapez une commande dans le terminal de l'hôte (par exemple pour définir `OLLAMA_API_KEY`), ce terminal peut la garder dans son historique.

## Clé en lecture seule : le mode éphémère

Si le dossier de cmd_ai n'est pas inscriptible (clé verrouillée en lecture seule, support protégé, dossier sans droits d'écriture), cmd_ai passe en **mode éphémère** :

- ni journaux, ni mémoire ;
- la mention « éphémère » est affichée **en permanence** dans la barre d'état ;
- cmd_ai ne se replie **jamais** sur un dossier de l'hôte pour écrire ailleurs.

Le reste fonctionne normalement. `config.json` doit déjà exister : il ne peut pas être créé sur un support en lecture seule.

## Retirer la clé USB

Quittez cmd_ai avant de retirer la clé, puis éjectez-la proprement (« Éjecter » sous Windows et macOS, « Démonter » ou « Éjecter » sous Linux) : les journaux et la mémoire sont écrits sur la clé, et un retrait brutal peut abîmer un système de fichiers exFAT.

## Binaires non signés : les blocages possibles

La V1 n'est **pas signée**. Selon la machine, le premier lancement peut être bloqué. Le scénario « je branche ma clé à la bibliothèque » n'est donc **pas garanti en V1**. La signature de code est prévue avant la 1.0.

### Windows

- **SmartScreen** (« Windows a protégé votre ordinateur ») : apparaît pour un fichier téléchargé, marqué comme venant d'Internet. Cliquez sur **Informations complémentaires**, puis **Exécuter quand même**. Vous pouvez aussi, avant de décompresser, ouvrir les **Propriétés** du zip et cocher **Débloquer**. Selon l'outil de décompression utilisé, la marque d'origine Internet est conservée ou non.
- **Smart App Control** (Windows 11, en mode application) : il bloque les programmes inconnus et non signés, sans bouton « exécuter quand même ». Sur votre propre PC, vous pouvez le désactiver dans **Sécurité Windows → Contrôle des applications et du navigateur** (il peut désormais être réactivé ensuite). Sur la machine de quelqu'un d'autre, ne touchez pas à ses réglages de sécurité.
- **AppLocker, WDAC ou stratégie « interdire l'exécution depuis les disques amovibles »** : sur un PC géré (entreprise, école, bibliothèque), l'administrateur peut interdire les programmes non approuvés ou toute exécution depuis une clé USB. Ces règles ne se contournent pas, et il ne faut pas essayer : demandez à l'administrateur.
- **Antivirus** : un exécutable non signé et peu répandu peut déclencher une alerte heuristique. Vérifiez l'empreinte SHA-256 du zip (voir [Installation](./installation.md)).

### macOS

- **Gatekeeper** : un binaire non signé par un développeur identifié et marqué « en quarantaine » (téléchargé) peut être refusé : « cmd_ai-macos ne peut pas être ouvert car le développeur ne peut pas être vérifié ». Deux solutions :
  1. ouvrez **Réglages Système → Confidentialité et sécurité**, puis cliquez sur **Ouvrir quand même** en face du message concernant cmd_ai (le clic droit puis « Ouvrir » ne suffit plus depuis macOS Sequoia) ;
  2. ou retirez l'attribut de quarantaine dans le Terminal :

```sh
xattr -d com.apple.quarantine /Volumes/CLE/cmd_ai/cmd_ai-macos
# ou pour tout le dossier
xattr -dr com.apple.quarantine /Volumes/CLE/cmd_ai
```

- **Accès au volume amovible** : au premier accès à la clé, macOS demande si le Terminal peut accéder aux fichiers d'un volume amovible. Répondez **Autoriser**. Cette autorisation est conservée sur le Mac.
- **Fichiers `._*`** : sur une clé exFAT, macOS crée des petits fichiers `._nom` qui contiennent les attributs étendus (dont la quarantaine). Ils sont sans danger ; c'est aussi pour cela que la quarantaine « suit » la clé d'un Mac à l'autre.

### Linux

- **FAT32 et `showexec`** : la plupart des bureaux montent les clés FAT32 avec l'option `showexec`, qui n'autorise l'exécution que des fichiers en `.exe`, `.com` ou `.bat`. Le binaire Linux ne peut alors pas tourner, même avec `cmd_ai.sh`. La solution : **formater la clé en exFAT** (voir [Installation](./installation.md)). Copier le dossier sur le disque de la machine fonctionnerait, mais écrirait cmd_ai sur l'hôte.
- **Montage `noexec`** : un administrateur peut imposer `noexec` aux supports amovibles. Là non plus, rien à contourner.
- **Bit d'exécution** : sur exFAT, les fichiers sont exécutables par défaut. Si vous avez décompressé le zip sur un autre système de fichiers et que le système répond « Permission non accordée », rendez les binaires exécutables : `chmod +x cmd_ai-linux-*`.

### Le lanceur `cmd_ai.sh`

`cmd_ai.sh` est un script `sh` POSIX qui détecte l'OS et le processeur, puis lance le bon binaire (`cmd_ai-linux-x86_64`, `cmd_ai-linux-aarch64` ou `cmd_ai-macos`) en lui transmettant vos options. Lancez-le avec `sh cmd_ai.sh` : ainsi, il n'a pas besoin d'être lui-même exécutable. Sous macOS, `cmd_ai.command` fait la même chose par un double-clic dans le Finder.

## En résumé

| Situation | Ce qui se passe |
| --- | --- |
| Clé exFAT, OS non géré | fonctionne ; au premier lancement, confirmez SmartScreen ou Gatekeeper |
| Clé en lecture seule | mode éphémère (ni journaux ni mémoire), signalé |
| Clé FAT32 sous Linux | le binaire Linux ne s'exécute pas : reformatez en exFAT |
| Windows avec Smart App Control actif | bloqué en V1 (non signé) |
| PC géré (AppLocker, interdiction des supports amovibles) | bloqué ; pas de contournement |
| Mac avec Gatekeeper | « Ouvrir quand même » ou `xattr -d com.apple.quarantine` |
