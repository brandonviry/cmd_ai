---
title: Tests manuels de release
description: La liste des vérifications manuelles à cocher avant chaque release de cmd_ai - élévation UAC, sudo interactif, premier lancement sous Gatekeeper, clé USB exFAT sur chaque OS, et test de fumée contre le vrai fournisseur.
---

La plupart des comportements de cmd_ai sont couverts par des tests automatisés, exécutés par la CI sur Windows, Linux et macOS. Certains ne s'automatisent pas : une fenêtre UAC, une invite `sudo` réelle, Gatekeeper, une vraie clé USB, un vrai modèle. Ils sont vérifiés **à la main, avant chaque release**, avec cette liste.

## Avant de commencer

- [ ] Le zip `cmd_ai-vX.Y.Z-portable.zip` vient des **artefacts de la CI** (jamais d'un build local).
- [ ] Son empreinte SHA-256 correspond à celle publiée.
- [ ] Une clé USB formatée en **exFAT** contient le dossier décompressé.
- [ ] `config.json` contient une **vraie clé API** de test (avec un petit solde), jamais commitée.
- [ ] Les journaux des tests précédents ont été vidés de `log/`.

Notez pour chaque campagne : version, date, testeur, machines utilisées (OS, version, architecture) et résultat de chaque case.

## Windows : élévation UAC

Sur un compte administrateur, dans une session **non élevée** :

- [ ] Demander une action sans danger qui exige des droits administrateur (par exemple « affiche l'état du chiffrement BitLocker des disques »). Le plan porte le badge `admin` et la barre d'état indique `utilisateur`.
- [ ] Accepter l'élévation : `memory/resume.json` est créé dans le dossier de cmd_ai, la fenêtre UAC apparaît.
- [ ] Confirmer l'UAC : une session cmd_ai élevée démarre, la barre d'état indique `admin`, la demande est reprise avec le même mode, le même modèle et le même répertoire courant ; `memory/resume.json` a disparu.
- [ ] La commande s'exécute et sa sortie s'affiche dans la session élevée ; la session non élevée s'est terminée.
- [ ] Recommencer en **refusant** l'UAC : rien n'est relancé, aucune commande n'est exécutée, le terminal reste utilisable.
- [ ] Recommencer en **refusant** l'élévation dans cmd_ai : la commande n'est pas exécutée.
- [ ] Une commande « utilisateur » qui échoue avec « Accès refusé » amène l'agent à expliquer le problème en français et à proposer l'élévation.

## Linux et macOS : `sudo` interactif

Sur un compte membre des administrateurs (`sudo`), après `sudo -k` pour vider le cache d'authentification :

- [ ] Demander une action sans danger qui exige `root` (par exemple « liste le contenu de /root » sous Linux, « liste le contenu de /var/root » sous macOS). Le plan porte le badge `admin`.
- [ ] Après validation, le TUI se suspend et `sudo -v` demande le mot de passe **dans le terminal**. Le saisir : le TUI reprend, la commande s'exécute en `sudo -n …`.
- [ ] Le mot de passe n'apparaît **nulle part** : ni à l'écran de cmd_ai, ni dans `log/`, ni dans `memory/`.
- [ ] Recommencer en tapant un **mauvais mot de passe** jusqu'à l'échec de `sudo -v` : la tâche s'arrête proprement, sans invite bloquée, et le terminal est restauré.
- [ ] Avec une authentification encore valide (`sudo -v` juste avant), la commande admin s'exécute sans redemander le mot de passe.

À faire sur Linux **et** sur macOS.

## macOS : premier lancement avec Gatekeeper

Sur un Mac qui n'a jamais lancé cmd_ai, avec le zip **téléchargé par un navigateur** (pour qu'il porte la quarantaine) :

- [ ] Lancer `cmd_ai.command` par un double-clic : noter le message de Gatekeeper.
- [ ] Le contournement par **Réglages Système → Confidentialité et sécurité → Ouvrir quand même** fonctionne et cmd_ai démarre.
- [ ] Sur un second Mac (ou après avoir remis la quarantaine), le contournement par `xattr -dr com.apple.quarantine` sur le dossier fonctionne.
- [ ] La demande d'accès du Terminal au volume amovible apparaît ; après **Autoriser**, cmd_ai lit et écrit dans son dossier.
- [ ] Les deux architectures : tester sur un Mac Apple Silicon **et** sur un Mac Intel (binaire universel).

## Clé USB exFAT sur chaque OS

Avec **la même clé**, successivement sur Windows, Linux et macOS :

- [ ] cmd_ai démarre depuis la clé (`cmd_ai.exe`, `sh cmd_ai.sh`, `cmd_ai.command`), lancé depuis un **autre répertoire courant** que la clé : il lit bien le `config.json` de la clé.
- [ ] Une demande simple (« affiche la date et l'heure ») va jusqu'au résultat, en mode plan (1 validation) puis en bypass (0 validation).
- [ ] Un plan de 4 commandes affiche 4 blocs, les ratios `1/4` à `4/4`, et produit 4 entrées dans le journal de la session.
- [ ] Un nouveau fichier de session apparaît dans `log/` **sur la clé**, et la mémoire (`cmd_ai memory add` via une demande « retiens que… ») est partagée d'un OS à l'autre.
- [ ] Rien n'a été écrit par cmd_ai dans le profil de l'utilisateur, `%APPDATA%`, `%LOCALAPPDATA%`, `%TEMP%`, `~/.config`, `~/Library` ou `/tmp` (comparer avant et après la session).
- [ ] Les accents s'affichent correctement, y compris dans les sorties de Windows PowerShell 5.1 et de `cmd` (« éàçù »).
- [ ] Échap pendant une commande longue tue la commande et ses processus enfants ; Ctrl+C deux fois quitte ; le terminal est intact.
- [ ] Clé passée en **lecture seule** (verrou matériel, ou montage en lecture seule) : cmd_ai démarre en mode éphémère, affiché en permanence, et n'écrit rien nulle part.
- [ ] Sous Linux, avec une clé **FAT32** : le blocage est constaté et le message ou la documentation indique la solution (exFAT).
- [ ] Sous Windows, premier lancement depuis un zip téléchargé : SmartScreen se contourne comme documenté dans [Portabilité](./portabilite.md).
- [ ] Clé débranchée **après** avoir quitté cmd_ai et éjecté la clé : aucun fichier abîmé au rebranchement.

## Test de fumée contre le vrai fournisseur

Obligatoire **après tout changement du modèle par défaut** (dans `config.example.json`), et conseillé avant chaque release : la qualité des appels d'outils varie d'un modèle à l'autre. Avec une vraie clé, hors CI :

- [ ] `cmd_ai --no-tui --prompt "affiche la date et l'heure"` : le modèle appelle `run_command`, le plan s'affiche, `o` l'exécute, la réponse finale est **en français**, code de sortie 0.
- [ ] Une demande en plusieurs étapes produit un plan complet (argument `plan`) et la progression `n/M`.
- [ ] Une commande qui échoue volontairement (« lis le fichier inexistant.txt ») amène une correction proposée, sans dépasser 2 corrections.
- [ ] Une demande purement textuelle (« traduis "good morning" en français ») donne une réponse sans commande.
- [ ] Chaque rôle de `config.example.json` (`--model fast`, `--model fr`) répond et sait appeler l'outil.
- [ ] Une clé invalide donne le message « clé » et le code 5 ; un `base_url` injoignable donne le message réseau et le code 5, sans aucune commande exécutée.
- [ ] La clé API n'apparaît dans aucun fichier de `log/` (rechercher sa valeur).

**Campagne 0.1.0 (29/10/2026)** : test de fumée contre Ollama Cloud réalisé par le propriétaire avec sa clé, sur clé USB — V1 validée.

## Après la campagne

- [ ] Toutes les cases sont cochées, ou chaque échec a sa fiche dans `bug/` et son issue GitHub (voir [Bugs](./bugs.md)).
- [ ] Les notes de version rappellent les limites de la V1 non signée et leurs contournements.
- [ ] La clé API de test est révoquée si elle a circulé.
