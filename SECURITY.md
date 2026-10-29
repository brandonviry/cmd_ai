# Politique de sécurité

cmd_ai exécute de vraies commandes sur la machine où on le lance, souvent celle de quelqu'un d'autre, et transporte une clé API qui n'expire pas. Une faille peut donc toucher un tiers : merci de la signaler **en privé**.

## Versions prises en charge

| Version | Correctifs de sécurité |
| --- | --- |
| Dernière version publiée | oui |
| Versions antérieures | non : mettez à jour |

## Signaler une faille

**N'ouvrez pas d'issue publique**, ni de discussion ou de pull request qui décrit la faille.

1. Ouvrez un **avis de sécurité privé** : onglet **Security** du dépôt, puis **Report a vulnerability**, ou directement <https://github.com/brandonviry/cmd_ai/security/advisories/new>.
2. Indiquez la version (`cmd_ai --version`), l'OS, le shell, les étapes de reproduction et l'impact, avec si possible une preuve de concept minimale et **non destructive**.
3. Ne joignez **jamais** de vraie clé API, ni la vôtre ni celle d'un autre : utilisez une clé factice (par exemple `cle-canari-0123`).

### Ensuite

- Un accusé de réception arrive dès que possible (objectif : 7 jours ; le projet est maintenu par une seule personne).
- La faille est confirmée puis corrigée dans l'avis privé (fork privé temporaire proposé par GitHub), avec un test de non-régression.
- **L'issue publique est créée avec le correctif**, en même temps que sa fiche [`bug/`](bug/README.md) et la release qui le contient. L'avis est alors publié (avec un identifiant CVE si nécessaire) et vous êtes crédité si vous le souhaitez.
- Merci de ne rien divulguer avant la publication du correctif (divulgation coordonnée, au plus tard 90 jours après le signalement, sauf accord).

## Périmètre

Sont notamment des failles :

- **Une fuite de la clé API** : la clé apparaît à l'écran, dans `log/` ou `memory/`, dans une erreur ou une panique, dans le contexte envoyé au modèle ou dans l'environnement d'une commande lancée (`OLLAMA_API_KEY` compris). Ou bien elle part vers une autre adresse que `provider.base_url`, en HTTP en clair vers autre chose qu'une adresse de bouclage, ou la vérification TLS peut être désactivée.
- **Une violation de la règle « rien sur l'hôte »** : cmd_ai écrit hors de son dossier (profil utilisateur, `%APPDATA%`, `~/.config`, `~/Library`, dossier temporaire, registre, trousseau), se replie sur un dossier de l'hôte quand le sien est en lecture seule, ou modifie de lui-même la configuration de l'hôte (PATH, ExecutionPolicy, variables persistantes).
- **Un contournement du mode plan ou des garde-fous par injection de prompt** : une sortie de commande, un nom ou un contenu de fichier, une note de `memory/`, un skill ou une réponse du modèle obtient l'exécution d'une commande sans validation en mode plan, l'exécution d'une commande hors du plan validé, le passage en mode bypass, le contournement d'un garde-fou, ou la création ou la modification d'un hook ou d'un skill.
- **Un processus lancé hors de la porte d'exécution unique**, par exemple une auto-commande `cmd_ai skill …` transmise à un shell, ou une traversée de chemin (`cmd_ai skill ../config`).
- **Une élévation de droits** sans le consentement de l'OS, ou un mot de passe qui transite par cmd_ai (modèle, `log/`, `memory/`, `config.json`).
- **Une séquence de contrôle du terminal** non neutralisée dans une sortie : changement de titre, presse-papiers (OSC 52), déplacement du curseur…

Hors périmètre :

- Une commande que vous avez **vous-même validée** en mode plan, ou lancée en mode bypass sans déclencher de garde-fou : c'est le fonctionnement prévu, d'où l'importance de lire le plan.
- Les traces que l'OS garde de lui-même (Prefetch, Amcache, SRUM, historique des périphériques USB…), documentées comme hors de portée de cmd_ai.
- L'accès physique à la clé USB : la clé API y est stockée en clair dans `config.json`. En cas de perte, révoquez-la sur <https://ollama.com/settings/keys>.
- Le blocage des binaires non signés de la V1 (SmartScreen, Smart App Control, Gatekeeper), documenté avec ses contournements.
- Les failles du provider (Ollama) ou des modèles eux-mêmes : signalez-les à leurs auteurs.
