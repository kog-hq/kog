# Mycelium — design v0

- **Date** : 2026-08-06
- **Statut** : en relecture
- **Périmètre v0** : parseur Rust d'un projet TypeScript vers un graphe fichiers/imports,
  CLI, rendu WebGL. Ni Tauri, ni clustering, ni IA.

> Le document est en français ; les identifiants, schémas et messages du produit sont
> en anglais, le projet étant destiné à une publication open source internationale.

---

## 1. Problème

Un développeur qui accumule des codebases n'a aucune vue d'ensemble de son propre code.
Les outils existants se répartissent en deux camps qui ne se parlent pas :

| Camp | Exemples | Ce qu'ils font | Ce qui manque |
| --- | --- | --- | --- |
| Cartographes | Graphify-Labs/graphify | Comprennent le code, produisent un graphe | Aucun pilotage, mono-projet, export HTML statique |
| Pilotes | Open Cowork, CloudCLI, agents-ui, OpenHands | Lancent des agents IA | Aucune compréhension du code |

Mycelium vise l'intersection : **voir son code et piloter ses agents dessus, dans la même
carte**. La v0 ne traite que la première moitié — et même pas en entier.

### Référence marché

`Graphify-Labs/graphify`, mesuré le 2026-08-06 via l'API GitHub :

- 103 112 étoiles, 10 020 forks, 819 issues ouvertes
- créé le 2026-04-03 → **4 mois**
- Python, Apache-2.0 (relicencié depuis MIT), Y Combinator S26

C'est la preuve que le marché existe, et le concurrent à battre.

---

## 2. Ce que Graphify mesure — et ne mesure pas

Relevé dans `BENCHMARKS.md` (màj 2026-07-05) et `docs/how-it-works.md` :

| Benchmark | Métrique | graphify | Concurrents |
| --- | --- | --- | --- |
| LOCOMO (n=300) | recall@10 | 0,497 | mem0 0,048 · supermemory 0,149 |
| LOCOMO (n=300) | QA accuracy | 45,3 % | supermemory 49,7 % · mem0 27,3 % |
| LongMemEval-S (n=50) | QA accuracy | 76 % | à égalité avec dense RAG |
| ERPNext (~1M LOC) | key-fact coverage | **82,0 %** vs 70,8 % (baseline grep+read) | — |

Le chiffre « code » (82,0 %) mesure un **effet en aval** — est-ce qu'un agent répond
mieux — sur **n = 6 questions**, à ~140K tokens par requête. Il ne dit rien de la
justesse du graphe lui-même.

Leur pipeline de code est pourtant de la même famille que le nôtre :

> « Tree-sitter parses your code files […] This runs locally with no LLM involved.
> 25 languages supported. »
> « Code files are **not** sent to the LLM semantic extractor in the normal pipeline. »

Chaque relation est taguée `EXTRACTED` (confiance 1.0), `INFERRED` (Claude, 0,55–0,95)
ou `AMBIGUOUS`. Pour le code, tout est `EXTRACTED` : déterministe, comme chez nous.

**Ce qu'ils ne publient jamais : quelle fraction des imports a été résolue**, ni au
total ni par langage. Un import raté ne remonte nulle part — il n'existe pas comme
échec, il disparaît du graphe.

C'est l'ouverture de Mycelium. Le taux de résolution coûte zéro à produire, se vérifie
repo par repo, et devient un argument public.

---

## 3. Décisions, et les mesures qui les ont dictées

### 3.1 Nœud = fichier, arête = import

Écarté : nœud = symbole, arête = appel. La résolution d'appels en TypeScript (alias,
réexports, dispatch dynamique, méthodes) tombe sous les 70 % d'exactitude sans
type-checker complet, et **un graphe faux est pire qu'un graphe grossier**.

Sur le plus gros projet de la machine de référence, le modèle fichier donne ~727 nœuds
et ~2 100 arêtes : lisible, vérifiable, suffisant pour prouver la chaîne Rust → WebGL.

### 3.2 TypeScript seul en v0

Recensement de la machine de référence, 2 209 fichiers de code :

| Extension | Fichiers | Part |
| --- | ---: | ---: |
| `.tsx` `.ts` `.js` `.jsx` | 2 097 | **94,9 %** |
| `.go` | 62 | 2,8 % |
| `.swift` | 34 | 1,5 % |
| `.rs` | 10 | 0,5 % |
| `.sh` | 6 | 0,3 % |

Mais la part n'est pas l'argument principal. Le modèle fichier+imports **ne s'applique
pas également à tous les langages** :

- **Swift — le modèle ne produit rien.** Les 34 fichiers du projet de référence importent
  exclusivement des frameworks système : `SwiftUI` ×23, `Foundation` ×12, `Combine` ×9,
  `WidgetKit` ×5, `StoreKit` ×3, `AVFoundation` ×2, `UserNotifications`, `Supabase`.
  **Zéro import interne.** C'est structurel : en Swift, les fichiers d'un même module se
  voient sans s'importer. Le graphe serait 34 nœuds et 0 arête. Swift attend le niveau
  symbole.
- **Go — applicable, mais le nœud change de nature.** 73 imports internes sur 298 au
  total, pointant vers des **packages** (répertoires), pas des fichiers :
  `ClientServer/internal/model` désigne 15 fichiers. Second modèle de nœud, second
  résolveur, pour ~18 nœuds.
- **Rust — 10 fichiers**, sans intérêt de démonstration.

### 3.3 Règle d'entrée d'un langage

> **Un langage entre dans Mycelium quand il passe sa propre gate de résolution — pas
> quand sa grammaire compile.**

Chaque langage supporté affiche son taux. Un langage dont le modèle ne produit pas
d'arêtes (Swift aujourd'hui) est documenté comme tel plutôt que listé à vide.

C'est la réponse directe aux 25 langages non mesurés de Graphify : moins de langages,
chacun avec un chiffre.

### 3.4 Résolution des alias : obligatoire

Répartition des 4 355 specifiers `from '…'` du projet de référence :

| Catégorie | Compte |
| --- | ---: |
| Alias internes (`@/` `@common/` `@modules/` `@mastore/` `@lib/`) | 2 651 |
| Relatifs (`./` `../`) | 558 |
| **Total interne** | **3 209** (73,7 %) |
| Externes (`@nestjs` 282, `react` 261, `lucide-react` 196, `next` 98, `@tanstack` 41…) | 1 146 |

Ces 1 146 specifiers externes se répartissent sur **77 paquets distincts**.

Un résolveur qui ignore les `paths` de tsconfig perd **82,6 % des arêtes internes**. La
lecture des tsconfig n'est donc pas une option annexe, c'est le cœur du parseur.

Le projet de référence est de surcroît un monorepo Turborepo : `workspaces: ["apps/*",
"packages/*"]`, un `tsconfig.base.json` racine et cinq tsconfig imbriqués, avec des
imports `@mastore/*` entre packages. C'est le cas le plus dur, donc le bon banc d'essai.

### 3.5 Dépendances externes : ignorées, mais comptées

Les nœuds sont exclusivement les fichiers du projet — la topologie reste celle du code.
Chaque nœud porte `external_deps: ["react", "next"]`, ce qui permettra plus tard de
filtrer (« quels fichiers dépendent de Prisma ? ») sans repasser le parseur ni polluer
le layout avec des hubs à 257 arêtes.

### 3.6 Renderer : sigma.js

| Bibliothèque | Version | Licence |
| --- | --- | --- |
| `sigma` + `graphology` | 3.0.3 / 0.26.0 | **MIT** |
| `@cosmograph/cosmos` | 3.4.1 | **CC-BY-NC-4.0** |

Cosmograph est en licence non-commerciale, non-OSI, incompatible avec un MIT+Apache.
L'écarter est une contrainte de licence, pas une préférence technique.

### 3.7 Forme du prototype : CLI avant Tauri

Le crate et le CLI produisent `graph.json` ; une page Vite+sigma le charge. Tauri
n'apparaît qu'après la gate, pour envelopper un crate déjà testé. Si le graphe ne sert à
rien, on l'apprend en heures plutôt qu'en jours.

---

## 4. Architecture

```
mycelium/
├── Cargo.toml                 workspace
├── crates/
│   ├── mycelium-graph/        lib — extraction, résolution, assemblage
│   └── mycelium-cli/          bin — mycelium scan <dir> -o graph.json
├── app/                       Vite + React + TS + sigma
└── docs/design/, docs/plans/
```

`mycelium-graph`, un module par rôle :

| Module | Responsabilité | Dépend de |
| --- | --- | --- |
| `model.rs` | `Graph` / `Node` / `Edge` / `Stats`, serde. Agnostique du langage, zéro logique | — |
| `extractor.rs` | Trait `Extractor` : `extensions()`, `extract(source) -> Vec<Specifier>`, `resolve(...)` | tsconfig |
| `discover.rs` | Parcours, respect de `.gitignore`, filtrage par extensions déclarées | — |
| `tsconfig.rs` | Lecture et fusion des tsconfig (`extends`, `paths`, `baseUrl`) — le cœur du parseur, 719 lignes | discover |
| `extractors/typescript.rs` | Grammaire tree-sitter TS/TSX, règles de résolution TS | extractor, tsconfig |
| `graph.rs` | Assemblage, déduplication, statistiques | tous |

Le trait `Extractor` existe dès le premier jour. Ajouter Go doit être **un fichier**,
pas un refactor — et c'est précisément ce que la v0.2 vérifiera.

### Dépendances Rust

`tree-sitter` 0.26.11 et `tree-sitter-typescript` 0.23.2. Ce dernier ne dépend que de
`tree-sitter-language ^0.1`, le shim d'ABI stable : les deux versions s'accordent sans
intervention.

---

## 5. Modèle de données

```jsonc
{
  "nodes": [
    {
      "id": "apps/frontend/src/lib/api.ts",   // chemin relatif à la racine scannée
      "path": "apps/frontend/src/lib/api.ts",
      "lang": "typescript",
      "loc": 143,
      "external_deps": ["react", "@tanstack/react-query"]
    }
  ],
  "edges": [
    { "source": "apps/frontend/src/app/page.tsx", "target": "apps/frontend/src/lib/api.ts", "kind": "import" }
  ],
  "stats": {
    "files_discovered": 727,
    "files_parsed": 727,
    "specifiers_total": 4375,
    "specifiers_internal": 3211,
    "resolved": 3160,
    "unresolved": 0,
    "excluded": 51,
    "resolution_rate": 1.0,
    "external_specifiers": 1164,
    "external_packages_distinct": 77,
    "failures": [],
    "diagnostics": [
      {
        "path": "apps/backend/prisma/seed-travaux.ts",
        "line": 2,
        "specifier": "../src/generated/prisma/client",
        "kind": "excluded"
      }
    ]
  }
}
```

Chiffres tels que mesurés sur la cible d'acceptation, voir
`docs/measurements/2026-08-06-v0-gate.md`.

`resolution_rate` = `resolved / (specifiers_internal - excluded)`. Les externes sont
hors du calcul : un `import react` n'a pas à pointer vers un fichier. `excluded` est
retiré du dénominateur pour la même raison : un specifier qui résout vers un vrai
fichier délibérément hors périmètre (gitignoré, dossier toujours exclu, ou extension
que l'extracteur ne revendique pas) n'est pas un échec du résolveur — le compter contre
le taux sous-évaluerait la qualité du résolveur au lieu de la mesurer. À l'inverse, un
fichier que l'outil lui-même n'a pas réussi à lire ou parser n'est jamais `excluded` :
c'est notre échec, pas une cible hors périmètre, donc il reste `unresolved` et continue
de peser sur le taux.

`diagnostics` (plafonné à `MAX_DIAGNOSTICS`, cf. `model.rs`) identifie, fichier et ligne
à l'appui, chaque specifier `unresolved` ou `excluded` — un compte seul n'est pas
auditable (§7). En contrepartie, il n'enregistre que le fait qu'un specifier a été
exclu, jamais *pourquoi* (gitignoré ? dossier toujours exclu ? extension non
revendiquée ?) : cette distinction n'existe qu'en le vérifiant à la main sur le disque
(voir la limite correspondante dans le document de mesure, §12).

---

## 6. Règles de résolution TypeScript

Appliquées dans l'ordre, première correspondance retenue :

1. **Relatif** — `./x`, `../x` résolus depuis le répertoire de l'importeur.
2. **Alias tsconfig** — table construite en suivant les chaînes `extends`, `paths`
   interprétés relativement à `baseUrl` (ou au répertoire du tsconfig si absent). Le
   tsconfig applicable est le plus proche en remontant l'arborescence.
3. **Package du workspace** — `package.json` racine, champ `workspaces` ; un specifier
   `@scope/pkg` correspondant à un package local est résolu vers son `main`/`exports`,
   à défaut vers son `index.ts`.
4. **Externe** — tout le reste. Enregistré dans `external_deps`, jamais en arête.

Pour toute cible résolue, on essaie dans l'ordre : chemin exact, puis `.ts`, `.tsx`,
`.js`, `.jsx`, puis `<dir>/index.{ts,tsx,js,jsx}`. Un specifier en `.js` est aussi
tenté en `.ts` (convention ESM/NodeNext).

### Cas mesurés sur le projet de référence

| Cas | Compte | Traitement |
| --- | ---: | --- |
| `import type` | 303 | Résolu normalement — pointe vers de vrais fichiers |
| Réexports `export … from` | 24 | Arête normale ; pas de traversée de barrel en v0 |
| Fichiers `index.ts` | 5 | Résolution de répertoire |
| Imports d'assets (`.png`) | 1 | Résolu (fichier réel trouvé sur disque), puis exclu — extension hors du périmètre revendiqué par l'extracteur TypeScript |
| `import()` dynamique | 0 | Hors périmètre v0 |

Ces chiffres montrent que la difficulté réelle se réduit aux alias et à l'extension
absente. Le reste est marginal.

---

## 7. Erreurs — jamais silencieuses

| Situation | Comportement |
| --- | --- |
| Racine absente ou illisible | **Échec immédiat**, code de sortie non nul |
| Fichier non parsable | Sauté, consigné dans `stats.failures`, le scan continue |
| Import non résolu | Compté dans `stats.unresolved`, jamais jeté en silence |
| tsconfig illisible ou invalide | Avertissement, résolution relative seule sur ce sous-arbre |

Aucun filtre ne doit *fail open* : un filtre qui ne peut pas s'appliquer exclut plutôt
que d'inclure au hasard.

---

## 8. Tests

Fixtures synthétiques minuscules, une par règle de résolution, sur le modèle de dejavu
(1 777 LOC, 51 tests) :

- import relatif, simple et remontant
- alias tsconfig, avec et sans `baseUrl`
- chaîne `extends` sur deux niveaux
- package de workspace monorepo
- résolution de répertoire vers `index.ts`
- extension absente et specifier `.js` → fichier `.ts`
- specifier non résoluble → compté, non fatal
- fichier non parsable → consigné, scan poursuivi

La surface testée est `extractors/typescript.rs`, pas le parcours de fichiers.

---

## 9. Gate d'acceptation

La v0 est « terminée » quand, et seulement quand :

1. `mycelium scan` sur le projet monorepo de référence (727 fichiers) affiche un
   **`resolution_rate` ≥ 0,95**, mesuré et imprimé, jamais estimé.
2. `mycelium scan` sur un projet simple (93 fichiers, alias `@/*`) produit un graphe
   cohérent.
3. La page sigma affiche le graphe du monorepo et reste fluide au pan/zoom.
4. CI verte : `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, gitleaks.

Tant que ces quatre points ne sont pas réunis, rien d'autre ne commence.

---

## 10. Hors périmètre v0

Règle d'arrêt, pas abandon. Aucune ligne de code sur : Tauri et `src-tauri/`, clustering
Leiden, multi-projets, chat IA et permissions, calques (code mort, code IA non relu,
activité récente), **graphe de sessions** — bien que le parseur de transcripts de dejavu
soit disponible et prêt —, jauge de quota, gestionnaire MCP, vérificateur de diff,
synchronisation CLAUDE.md.

Chacun de ces morceaux est plus stimulant que la résolution d'alias, ce qui est
exactement pourquoi ils feraient dérailler la v0. Si l'un devient nécessaire en cours de
route, la question est posée avant d'être tranchée.

---

## 11. Après la gate

| Version | Contenu | Ce que ça prouve |
| --- | --- | --- |
| v0.2 | Extracteur Go (nœud = package) | Que le trait `Extractor` tient sur un second modèle de nœud |
| v0.3 | Coque Tauri autour du crate | Que le cœur se distribue en binaire unique |
| ensuite | Multi-projets, Leiden, calques, pilotage | La vision complète |

Swift n'entrera qu'avec le niveau symbole, et sera documenté comme tel d'ici là.

---

## 12. Réemploi de dejavu

À récupérer depuis `~/apps/dejavu` : licences MIT + Apache-2.0, `.gitleaks.toml`,
`rust-toolchain.toml`, workflow CI, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`,
`SECURITY.md`, templates d'issues et de PR.

Le parseur de transcripts (`adapters/claude_code.rs`, 278 lignes, 9 tests) alimentera le
graphe de sessions — **après** la v0.

---

## 13. v0.1 — `mycelium` sans argument

Premier retour sur la v0 une fois la gate passée : voir un graphe exigeait trois
commandes, un clone du dépôt et `bun` — `mycelium scan ~/projet -o
app/public/graph.json`, puis `cd app && bun run dev`. Ça contredit directement le
différenciateur affiché face au concurrent (§3.7, §11) : « un seul binaire, zéro
dépendance », alors que le concurrent Python mesuré au §1 a des issues pleines de
douleur d'installation. Exiger un checkout du dépôt et une chaîne JS pour voir le
résultat concède exactement ce que mycelium prétend éviter. Cette feature est donc sur
la trajectoire du projet, pas un confort ajouté.

Décisions :

- `ROOT` a pour valeur par défaut `.` sur `scan` comme sur `view` — taper un chemin
  devient optionnel partout.
- `mycelium` sans sous-commande équivaut explicitement à `mycelium view .`, câblé dans
  le setup clap plutôt que laissé à un comportement implicite.
- `--stats-only` disparaît : `scan` n'écrit un fichier que si `-o` est donné, plutôt que
  d'écrire par défaut et de proposer un drapeau pour s'en abstenir. Même comportement,
  moins de surface.
- `view` ne touche jamais le disque : le graphe est gardé en mémoire et servi tel quel.
  Lancer `mycelium` dans son projet ne doit jamais laisser un `graph.json` derrière soi.

La page (`app/dist`, produite par `bun run build`) est embarquée dans le binaire via
`rust-embed`, servie par `tiny_http` — synchrone, donc aucun runtime async n'entre dans
le binaire — et le navigateur est ouvert via `open`. Le serveur écoute sur `127.0.0.1`
uniquement (c'est la structure du code source de l'utilisateur qui est servie ; jamais
`0.0.0.0`), sur un port choisi par l'OS (bind sur le port 0, lu après coup) plutôt qu'un
port fixe comme 4173/5173, qui pourrait déjà être pris. L'URL est imprimée avant
l'ouverture du navigateur, pour rester utile en SSH ou si l'ouverture échoue.

`crates/mycelium-cli/build.rs` vérifie que `app/dist/index.html` existe avant de
compiler et échoue avec la commande exacte pour le produire (`cd app && bun install &&
bun run build`) plutôt que de laisser la macro `rust-embed` échouer avec un « file not
found » qui n'oriente vers rien. Il émet aussi `cargo:rerun-if-changed` sur `app/dist` :
sans ça, une page reconstruite resterait embarquée obsolète dans le prochain binaire
compilé — le mode d'échec le plus coûteux à découvrir tard.

---

## Annexe — environnement de référence

Machine de développement, vérifiée le 2026-08-06 :

- cargo 1.97.1, toolchain `stable-aarch64-apple-darwin`.
  `~/.cargo/bin` **absent du PATH non interactif** — à corriger avant le premier build.
- node v22.23.2, bun 1.3.8
- Xcode CLT présents
- `gh` authentifié sur le compte cible
