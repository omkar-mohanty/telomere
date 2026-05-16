{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { flake-utils, naersk, nixpkgs, rust-overlay,... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
      overlays = [ (import rust-overlay) ];
        pkgs = (import nixpkgs) {
          inherit system overlays;
        };


        naersk' = pkgs.callPackage naersk {};

      in {
        # For `nix build` & `nix run`:
        packages.default = naersk'.buildPackage {
          src = ./.;
          nativeBuildInputs = with pkgs; [ rust-bin.beta.latest.default ];
        };

        # For `nix develop`:
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ rust-bin.beta.latest.default ];
        };
      }
    );
}
