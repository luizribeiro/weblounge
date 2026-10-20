{ lib, stdenvNoCC, runCommand, writeShellApplication, writeShellScript, gawk, diffutils, esbuild, python3, chromium, imagemagick }:

let
  src = ../web;

  bundles = {
    # Hashed names aren't worth a templating step for one static page.
    landing = {
      entryPoints = [ "scripts/landing.ts" "styles/landing.css" "images/wordmark.svg" ];
      entryNames = "[name]";
    };
    # Concierge looks the hashed names up in esbuild's metafile.
    members = {
      entryPoints = [ "styles/members.css" "scripts/members.ts" "images/favicon.svg" "images/wordmark.svg" "images/chair.svg" ];
      entryNames = "[name]-[hash]";
    };
  };

  esbuildCommand = { dev }: bundle: flags: lib.escapeShellArgs (
    [ "esbuild" ] ++ bundle.entryPoints ++ [
      "--bundle"
      "--entry-names=${bundle.entryNames}"
      "--loader:.svg=copy"
      "--loader:.png=file"
      "--loader:.avif=file"
      "--target=es2022,chrome100,firefox100,safari15"
      "--define:DEV=${lib.boolToString dev}"
    ] ++ flags
  );

  # Static pages inline SVGs, e.g. the chair the landing page's script
  # animates and concierge's home shares: `<!-- include images/chair.svg -->`
  # in the page given becomes that file, indented like the comment. Run
  # from web/.
  renderPage = writeShellScript "render-page" ''
    ${gawk}/bin/awk '
      $1 == "<!--" && $2 == "include" && $4 == "-->" {
        match($0, /^ */)
        indent = substr($0, 1, RLENGTH)
        while ((getline line < $3) > 0) print (line == "" ? "" : indent line)
        close($3)
        next
      }
      { print }
    ' "$1"
  '';

  release = esbuildCommand { dev = false; };
  development = esbuildCommand { dev = true; };
in
stdenvNoCC.mkDerivation {
  name = "weblounge-web";
  inherit src;

  nativeBuildInputs = [ esbuild ];

  buildPhase = ''
    runHook preBuild
    ${release bundles.landing [ "--minify" "--outdir=dist/assets" ]}
    ${renderPage} pages/index.html > dist/index.html
    install -Dm644 images/favicon.svg dist/favicon.svg
    ${release bundles.members [ "--minify" "--outdir=members" "--metafile=members/manifest.json" ]}
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r dist $out/landing
    cp -r members $out/members
    runHook postInstall
  '';

  passthru = {
    # Redraws web/images/felt-* from web/felt, given the path to web/.
    bakeFelt = writeShellApplication {
      name = "weblounge-bake-felt";
      runtimeInputs = [ python3 chromium imagemagick ];
      text = ''
        python3 "$1/felt/bake.py"
        python3 "$1/felt/seams.py" "$1"/images/felt-*
      '';
    };

    feltSeams = runCommand "felt-seams" { nativeBuildInputs = [ python3 imagemagick ]; } ''
      python3 ${src}/felt/seams.py ${src}/images/felt-*
      touch $out
    '';

    # Serves the working tree, not the store copy, so edits show up live.
    dev = writeShellApplication {
      name = "weblounge-web-dev";
      runtimeInputs = [ esbuild diffutils ];
      text = ''
        cd "$1"
        served=$(mktemp -d)
        trap 'kill $(jobs -p) 2>/dev/null; rm -rf "$served"' EXIT
        ln -s "$PWD/images/favicon.svg" "$served/favicon.svg"
        render() {
          ${renderPage} pages/index.html > "$served/next.html"
          cmp -s "$served/next.html" "$served/index.html" || mv "$served/next.html" "$served/index.html"
        }
        render
        while sleep 1; do render; done &
        ${development bundles.landing [ "--sourcemap" "--watch=forever" ]} \
          --outdir="$served/assets" --servedir="$served" --serve=127.0.0.1:"$2"
      '';
    };

    # Concierge serves these itself, restarting whenever the manifest changes.
    watchMembers = writeShellApplication {
      name = "weblounge-members-watch";
      runtimeInputs = [ esbuild ];
      text = ''
        out=$(realpath -m "$2")
        cd "$1"
        exec ${development bundles.members [ "--sourcemap" "--watch=forever" ]} \
          --outdir="$out" --metafile="$out/manifest.json"
      '';
    };
  };
}
