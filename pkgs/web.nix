{ lib, stdenvNoCC, runCommand, writeShellApplication, esbuild, typescript }:

let
  src = ../web;

  bundles = {
    # Hashed names aren't worth a templating step for one static page.
    landing = {
      entryPoints = [ "scripts/landing.ts" "styles/landing.css" ];
      entryNames = "[name]";
    };
    # Concierge looks the hashed names up in esbuild's metafile.
    members = {
      entryPoints = [ "styles/members.css" "images/favicon.svg" ];
      entryNames = "[name]-[hash]";
    };
  };

  esbuildCommand = { dev }: bundle: flags: lib.escapeShellArgs (
    [ "esbuild" ] ++ bundle.entryPoints ++ [
      "--bundle"
      "--entry-names=${bundle.entryNames}"
      "--loader:.svg=copy"
      "--target=es2022,chrome100,firefox100,safari15"
      "--define:DEV=${lib.boolToString dev}"
    ] ++ flags
  );

  release = esbuildCommand { dev = false; };
in
stdenvNoCC.mkDerivation {
  name = "weblounge-web";
  inherit src;

  nativeBuildInputs = [ esbuild ];

  buildPhase = ''
    runHook preBuild
    ${release bundles.landing [ "--minify" "--outdir=dist/assets" ]}
    install -Dm644 pages/index.html dist/index.html
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
    typecheck = runCommand "weblounge-web-typecheck"
      { nativeBuildInputs = [ typescript ]; }
      ''
        tsc -p ${src}/tsconfig.json
        touch $out
      '';

    # Serves the working tree, not the store copy, so edits show up live.
    dev = writeShellApplication {
      name = "weblounge-web-dev";
      runtimeInputs = [ esbuild ];
      text = ''
        cd "$1"
        served=$(mktemp -d)
        trap 'rm -rf "$served"' EXIT
        ln -s "$PWD/pages/index.html" "$served/index.html"
        ln -s "$PWD/images/favicon.svg" "$served/favicon.svg"
        ${esbuildCommand { dev = true; } bundles.landing [ "--sourcemap" "--watch=forever" ]} \
          --outdir="$served/assets" --servedir="$served" --serve=127.0.0.1:"$2"
      '';
    };
  };
}
