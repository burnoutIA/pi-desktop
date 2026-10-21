# Extensions pi livrées avec l'app

Ce dossier ne contient **que** les extensions livrées avec Pi Voice. Elles sont suivies par git et
l'app les passe à pi avec `--extension` à chaque lancement : chaque fichier `*.ts` et chaque
sous-dossier qui a un `index.ts`.

| Extension | Rôle |
|---|---|
| `preview.ts` | aperçu de fichiers dans l'app |
| `ctx-optimizer/` | allège le contexte envoyé au modèle (repris de [burnoutIA/ctx_optimizer](https://github.com/burnoutIA/ctx_optimizer)) |

## Tes propres extensions ne vont pas ici

Une extension utile seulement pour toi va dans les extensions globales du pi de l'app,
`data/pi-agent/extensions/`, qui n'est pas suivi par git. Pi la charge tout seul. C'est aussi là
que pi range les extensions qu'il crée ou installe.

À ne pas confondre avec `.pi/extensions/` d'un projet, qui ne sert qu'à ce projet.
