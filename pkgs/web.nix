{ lib, stdenvNoCC, runCommand, writeShellApplication, esbuild, typescript }:

let
  src = ../web;

  esbuildCommand = { dev }: flags: lib.escapeShellArgs ([
    "esbuild"
    "scripts/landing.ts"
    "styles/landing.css"
    "--bundle"
    "--entry-names=[name]"
    "--target=es2022,chrome100,firefox100,safari15"
    "--define:DEV=${lib.boolToString dev}"
  ] ++ flags);
in
stdenvNoCC.mkDerivation {
  name = "weblounge-web";
  inherit src;

  nativeBuildInputs = [ esbuild ];

  buildPhase = ''
    runHook preBuild
    ${esbuildCommand { dev = false; } [ "--minify" "--outdir=dist/assets" ]}
    install -Dm644 pages/index.html dist/index.html
    install -Dm644 images/favicon.svg dist/favicon.svg
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -r dist $out/landing
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
        ${esbuildCommand { dev = true; } [ "--sourcemap" "--watch=forever" ]} \
          --outdir="$served/assets" --servedir="$served" --serve=127.0.0.1:"$2"
      '';
    };
  };
}
