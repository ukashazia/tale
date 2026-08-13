{
  description = "Tale release package";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      version = "2.0.5";
      releases = {
        aarch64-darwin = {
          target = "aarch64-apple-darwin";
          hash = "sha256-B+S4T4tNHVKqSca3tboelr4EXezY9K/xvYJyMevDqrU=";
        };
        aarch64-linux = {
          target = "aarch64-unknown-linux-gnu";
          hash = "sha256-DBnkI+MSEEELX1UtEg/5gS84ZQX5XSxbZ0T2ugHNErs=";
        };
        x86_64-darwin = {
          target = "x86_64-apple-darwin";
          hash = "sha256-i/mFsxqRMD7/luOk0gT+YFdu58wF7OjdQtYOw/CtJtE=";
        };
        x86_64-linux = {
          target = "x86_64-unknown-linux-gnu";
          hash = "sha256-anGSVpp3sc7zkA+kE6Dx9KECwmqGwSxyjki8dpaCidc=";
        };
      };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          release = releases.${system};
        in
        {
          default = pkgs.stdenvNoCC.mkDerivation {
            pname = "tale";
            inherit version;
            src = pkgs.fetchurl {
              url = "https://github.com/ukashazia/tale/releases/download/v${version}/tale-${release.target}.tar.gz";
              inherit (release) hash;
            };
            sourceRoot = "tale-${release.target}";
            dontBuild = true;
            installPhase = ''
              runHook preInstall
              install -Dm755 tale $out/bin/tale
              install -Dm644 docs/cli/tale.1 $out/share/man/man1/tale.1
              install -Dm644 completions/tale.bash $out/share/bash-completion/completions/tale
              install -Dm644 completions/_tale $out/share/zsh/site-functions/_tale
              install -Dm644 completions/tale.fish $out/share/fish/vendor_completions.d/tale.fish
              runHook postInstall
            '';

            meta = {
              description = "A keyboard-first terminal application for Tailscale networks";
              mainProgram = "tale";
              license = pkgs.lib.licenses.mit;
              platforms = systems;
              sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
            };
          };
        });
    };
}
