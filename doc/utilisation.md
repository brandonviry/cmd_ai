---
title: Utilisation
description: Se servir de cmd_ai au quotidien, dans le TUI interactif ou en une seule commande (one-shot) - saisie, plan, touches, commandes /aide, /model et autres, progression et affichage des commandes.
---

Vous parlez, cmd_ai traduit en commandes et les exécute. Cette page décrit les deux façons de travailler : le **TUI interactif** (TUI ouvert) et le **one-shot** (une demande, puis fin).

## Le TUI interactif

Lancez cmd_ai sans argument :

```text
cmd_ai
```

Le TUI s'ouvre **dans le terminal courant**, sans écran alternatif : les commandes terminées restent dans l'historique du terminal, et vous les retrouvez en remontant, même après avoir quitté.

### La ligne de saisie

Tapez votre demande en français, en langage naturel, puis **Entrée** :

```text
> trouve les fichiers .log de plus de 100 Mo dans mon dossier personnel
```

Vous n'avez pas à connaître le shell de la machine : cmd_ai indique au modèle l'OS, l'architecture, le shell détecté et sa version, ainsi que votre niveau de droits.

Le modèle peut aussi répondre sans exécuter de commande (« traduis-moi ça », « explique-moi ce message d'erreur ») : la réponse s'affiche, simplement.

Au sein d'une même session, cmd_ai garde l'historique de la conversation : vous pouvez enchaîner (« et maintenant supprime-les »). `/effacer` le vide.

### La barre d'état

Elle affiche en permanence :

- le **mode** actif (`plan` ou `bypass`, voir [Modes](./modes.md)) ;
- le **niveau de droits** de la session (`utilisateur` ou `admin`, voir [Droits](./droits.md)) ;
- le **modèle** utilisé ;
- la mention **éphémère** si le dossier de cmd_ai n'est pas inscriptible (ni journaux ni mémoire, voir [Portabilité](./portabilite.md)).

Chaque information porte un libellé ou un symbole, pas seulement une couleur : le TUI reste lisible sans couleur.

### Le plan et la validation

En mode plan (le défaut), l'agent présente d'abord la suite complète des commandes prévues, numérotée, avec pour chacune le shell, le niveau de droits et une explication :

```text
Plan (3 commandes) :
  1. bash · utilisateur — Créer le dossier de sauvegarde
     mkdir -p ~/sauvegardes
  2. bash · utilisateur — Compresser le dossier Documents
     tar -czf ~/sauvegardes/documents.tar.gz -C ~ Documents
  3. bash · utilisateur — Afficher la taille de l'archive
     ls -lh ~/sauvegardes/documents.tar.gz
Exécuter ce plan ? [o/N]
```

(Rendu simplifié.) Rien ne s'exécute avant votre réponse :

| Touche | Effet |
| --- | --- |
| `o` | valide : le plan s'exécute |
| `n` | refuse : rien n'est exécuté, le modèle est informé du refus et peut vous proposer autre chose |

Une commande qui ne figure pas dans le plan validé (une correction après un échec, une étape ajoutée) vous est **redemandée** avant de tourner. Les détails sont dans [Modes](./modes.md).

### Les touches

| Touche | Effet |
| --- | --- |
| **Entrée** | envoie la demande |
| `o` / `n` | valide ou refuse le plan ou la commande proposée |
| **Échap** | annule la tâche en cours : la commande et tout son arbre de processus sont tués, la main revient à la saisie |
| **Ctrl+C** | annule la tâche en cours ; un second appui quitte cmd_ai |
| **Ctrl+D** | quitte cmd_ai (comme `/quitter`) |

À la sortie, même après une erreur, cmd_ai restaure le terminal dans son état d'origine.

### Les commandes du TUI

Elles commencent par `/` et se tapent dans la ligne de saisie :

| Commande | Effet |
| --- | --- |
| `/aide` | affiche l'aide (commandes et touches) |
| `/plan` | passe en mode plan (validation avant exécution) |
| `/bypass` | passe en mode bypass (exécution sans confirmation, sauf garde-fous) |
| `/model fast` | utilise le modèle du rôle `fast` de `config.json` |
| `/model gpt-oss:120b` | utilise ce modèle précis |
| `/model` | revient au modèle par défaut (`provider.model`) |
| `/effacer` | oublie l'historique de la conversation (pas la [mémoire](./memoire.md)) |
| `/quitter` | quitte cmd_ai |

Le changement de mode ou de modèle s'affiche aussitôt dans la barre d'état. Un rôle inconnu affiche la liste des rôles disponibles et ne change rien. La résolution des noms de modèles est décrite dans [Configuration](./configuration.md).

### Le raisonnement du modèle

Certains modèles produisent un raisonnement avant leur réponse. Avec `"ui": { "show_reasoning": true }`, le TUI l'affiche en atténué pendant qu'il arrive. Il n'est pas conservé dans l'historique une fois la réponse donnée.

## Une demande en une seule commande (one-shot)

### `--prompt` et `--prompt-file`

```text
cmd_ai --prompt "liste les fichiers les plus gros de ce dossier"
cmd_ai --prompt-file demande.txt
```

- Le texte (ou le fichier) est lu **avant toute interface**.
- `--prompt-file` lit un fichier texte UTF-8 ; un chemin relatif part du répertoire courant.
- `--prompt` et `--prompt-file` ne se combinent pas (erreur d'usage, code 2).
- Dans un terminal interactif, la tâche tourne **TUI ouvert** : vous la suivez en direct, puis le TUI reste disponible pour une nouvelle demande.

### `--no-tui` : TUI fermé

```text
cmd_ai --no-tui --prompt "affiche la date et l'heure"
```

Avec `--no-tui`, cmd_ai écrit tout en texte simple sur la sortie standard (et les erreurs sur la sortie d'erreur), exécute **une seule tâche**, puis s'arrête avec un [code de sortie](./codes-de-sortie.md) qui dit si la tâche a réussi.

- En mode plan, le plan s'affiche et cmd_ai demande `Exécuter ce plan ? [o/N]` : tapez `o` puis Entrée. Toute autre réponse refuse.
- Si l'entrée standard n'est pas un terminal (script, tâche planifiée, entrée redirigée), personne ne peut valider : cmd_ai affiche le plan, **n'exécute rien** et sort avec le code 3. Pour un script, utilisez `--bypass` en connaissance de cause (voir [Modes](./modes.md)).
- Si la sortie standard n'est pas un terminal (redirection vers un fichier, tube), cmd_ai passe automatiquement en TUI fermé.
- Sans `--prompt` ni `--prompt-file`, et sans terminal interactif, cmd_ai ne peut rien faire : erreur d'usage, code 2.

Exemple pour un script :

```text
cmd_ai --no-tui --bypass --prompt "compresse le dossier photos en photos.zip"
```

## Ce qui s'affiche pour chaque commande

Avant de tourner, chaque commande est affichée avec :

- sa position dans la tâche (`2/4`) et, pour une suite, le pourcentage ;
- le **shell** utilisé (`pwsh`, `powershell`, `cmd`, `bash`, `sh` ou `zsh`) ;
- le **niveau de droits** (`utilisateur` ou `admin`) ;
- l'**explication** en français ;
- le **texte exact** de la commande.

Puis, pendant qu'elle tourne, sa sortie (standard et erreur) **en direct**. À la fin : son **code de sortie** et sa **durée**, et le cas échéant les mentions « délai dépassé », « annulé » ou « sortie tronquée ».

Pour une suite de commandes, la progression s'affiche en ratio `n/M` : `1/4`, puis `2/4`… Si le plan change en cours de route (une commande ajoutée), `M` est mis à jour visiblement.

Les commandes lancées par les [hooks](./hooks.md) s'affichent de la même façon, marquées `[hook]`.

Une fois la tâche finie, le modèle donne sa réponse en français.

## Comment l'agent enchaîne

- **Après un échec**, l'agent lit la sortie et propose une correction (en mode plan, vous la validez ; en bypass, il la tente). Au plus **2 corrections** pour une même commande, puis il s'arrête et vous explique.
- **Au plus 30 étapes** par tâche : au-delà, la tâche s'arrête et vous rend la main, quel que soit le mode.
- **Une commande à la fois** : les commandes d'une suite s'exécutent dans l'ordre, jamais en parallèle.

## Cas particuliers

- **Commande qui attend une saisie** : l'entrée standard des commandes est fermée. Une commande qui attend malgré tout est arrêtée au bout du délai (120 s par défaut, `limits.command_timeout_secs`), avec tout son arbre de processus. Quand une commande a vraiment besoin de votre clavier, l'agent peut demander une étape « interactive » : cmd_ai vous rend alors le terminal le temps de cette commande.
- **Application graphique** (`start`, `Start-Process` sans `-Wait`, `Invoke-Item`, `open`, `xdg-open`) : elle est lancée détachée ; cmd_ai n'attend pas que vous la fermiez. « Ouvre YouTube » ouvre le navigateur, et l'agent continue.
- **Sortie énorme** : la capture est plafonnée à 1 Mio par flux, et le modèle n'en reçoit que 16 Kio (le début et la fin). La troncature vous est signalée, ainsi qu'au modèle.
- **Accents** : les sorties de Windows PowerShell 5.1 et de `cmd` (pages de code OEM) sont décodées correctement.
- **Séquences de contrôle** dans une sortie (changement de titre, presse-papiers, déplacement du curseur) : elles sont neutralisées avant affichage ; seules les couleurs passent.

## Le choix du shell

cmd_ai choisit le shell de façon déterministe :

- **Windows** : PowerShell 7 (`pwsh`) s'il est présent, sinon Windows PowerShell 5.1 ; `cmd` seulement si la commande le demande.
- **Linux et macOS** : `bash`, sinon `sh` ; `zsh` seulement si la commande le demande.

Vous pouvez simplement le demander : « fais-le en cmd ». PowerShell est toujours lancé sans profil (`-NoProfile -NonInteractive`).

Les commandes s'exécutent dans le **répertoire courant** du terminal d'où vous avez lancé cmd_ai.

## Toutes les options

```text
cmd_ai [OPTIONS]
  --prompt <texte>        exécute cette demande (lue avant toute interface)
  --prompt-file <fichier> idem, depuis un fichier UTF-8 (chemin relatif au répertoire courant)
  --no-tui                TUI fermé : sortie texte, une seule tâche, puis fin
  --tui                   TUI ouvert même si config.ui.tui = false
  --plan | --bypass       mode de permissions (défaut : config, sinon plan)
  --model <rôle|nom>      modèle à utiliser
  --home <dossier>        dossier cmd_ai (sinon CMD_AI_HOME, sinon dossier de l'exécutable)
  --color <auto|truecolor|256|16|none>
  --resume <fichier>      reprend une tâche sauvegardée (relance élevée sous Windows)
  -V, --version           affiche « cmd_ai X.Y.Z »
  -h, --help              aide en français
```

- `--resume` sert à la relance en administrateur sous Windows (voir [Droits](./droits.md)) : vous n'avez pas à l'utiliser vous-même.
- `--home` (ou la variable `CMD_AI_HOME`) désigne un autre dossier cmd_ai que celui de l'exécutable.
- `--color none` (ou la variable `NO_COLOR`) désactive les couleurs.

## Les journaux

Hors mode éphémère, chaque session est journalisée dans `log/`, dans un fichier `AAAA-MM-JJ_HHMMSS.log` : horodatage (UTC), commande, shell, droits, sortie, code et durée. La clé API y est masquée. Un fichier ne dépasse pas 10 Mio, et le dossier 50 Mio : au-delà, les plus anciens sont supprimés.
