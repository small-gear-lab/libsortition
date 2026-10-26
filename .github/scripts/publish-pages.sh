#!/usr/bin/env bash
# bindings/wasm のビルド結果を、gh-pages ブランチへ公開する。
#
#   KIND=next    NAME=next    COMMIT=<SHA> REMOTE=<URL>                 → /next/ を置き換える
#   KIND=release NAME=vX.Y.Z  COMMIT=<SHA> REMOTE=<URL> STABLE=true|false
#                                         → /vX.Y.Z/ を作る（すでにあれば触らない）。
#                                           STABLE=true で、そのバージョンが最新の安定版なら /latest/ も置き換える
#
# bindings/wasm/sortition.wasm はビルド済みであること（bindings/wasm/build.sh）。
# gh-pages の履歴は使い捨てで、公開のたびに 1 コミットだけのブランチに作り直して force push する
# （ビルド結果が履歴に積もって、リポジトリが太るのを防ぐ）。バージョンのフォルダはツリーに残る。
# push には --force-with-lease を使い、読んだあとに誰かが書き込んでいたら失敗する。
set -euo pipefail

: "${KIND:?KIND is required (next or release)}"
: "${NAME:?NAME is required}"
: "${COMMIT:?COMMIT is required}"
: "${REMOTE:?REMOTE is required}"
STABLE="${STABLE:-false}"

ROOT="$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

case "$KIND" in
  next) [ "$NAME" = next ] || { echo "NAME must be 'next' for KIND=next" >&2; exit 1; } ;;
  release) [[ "$NAME" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "bad version name: $NAME" >&2; exit 1; } ;;
  *) echo "unknown KIND: $KIND" >&2; exit 1 ;;
esac

python3 "$ROOT/.github/scripts/pages.py" assemble --repo "$ROOT" --dest "$WORK/site" --name "$NAME" --commit "$COMMIT"

git clone --quiet --depth 1 --branch gh-pages "$REMOTE" "$WORK/pages"
cd "$WORK/pages"
git config user.name "${GIT_USER_NAME:-github-actions[bot]}"
git config user.email "${GIT_USER_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"
OLD="$(git rev-parse HEAD)"
touch .nojekyll

if [ "$KIND" = next ]; then
  rm -rf next
  cp -R "$WORK/site" next
  SUBJECT="Publish next (${COMMIT:0:7})"
else
  if [ -e "$NAME" ]; then
    echo "$NAME is already published; leaving it as it is"
  else
    cp -R "$WORK/site" "$NAME"
  fi
  if [ "$STABLE" = true ]; then
    # 公開済みの安定版（プレリリースを除く）のうち、一番新しいのがこの版のときだけ latest を更新する。
    # 古い版をあとから出しても、latest が戻らないようにするため
    newest="$(for d in v[0-9]*; do [ -d "$d" ] || continue; case "$d" in *-*) continue ;; esac; echo "$d"; done | sort -V | tail -n 1)"
    if [ "$newest" = "$NAME" ]; then
      rm -rf latest
      cp -R "$NAME" latest
    else
      echo "$NAME is not the newest stable release ($newest); latest is left as it is"
    fi
  fi
  SUBJECT="Publish $NAME"
fi

python3 "$ROOT/.github/scripts/pages.py" index --root .

git add -A
if git diff --cached --quiet; then
  echo "Nothing to publish"
  exit 0
fi

git checkout --quiet --orphan publish
git commit --quiet -m "$SUBJECT" -m "Built from $COMMIT."
git push --quiet --force-with-lease="gh-pages:$OLD" origin publish:gh-pages
echo "Published: $SUBJECT"
