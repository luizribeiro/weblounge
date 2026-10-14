{ lib, stdenvNoCC, runCommand, esbuild, typescript }:

let
  src = ../web;

  esbuildCommand = flags: lib.escapeShellArgs ([
    "esbuild"
    "scripts/landing.ts"
    "styles/landing.css"
    "--bundle"
    "--entry-names=[name]"
    "--target=es2022,chrome100,firefox100,safari15"
  ] ++ flags);
in
stdenvNoCC.mkDerivation {
  name = "weblounge-web";
  inherit src;

  nativeBuildInputs = [ esbuild ];

  buildPhase = ''
    runHook preBuild
    ${esbuildCommand [ "--minify" "--outdir=dist/assets" ]}
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

  passthru.typecheck = runCommand "weblounge-web-typecheck"
    { nativeBuildInputs = [ typescript ]; }
    ''
      tsc -p ${src}/tsconfig.json
      touch $out
    '';
}
