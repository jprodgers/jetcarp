# JETCARP's Homebrew formula. This file is the source of truth: the release workflow fills in the
# version and the archives' checksums (packaging/package.py manifests) and writes the result to
# the tap, github.com/jprodgers/homebrew-jetcarp, as Formula/jetcarp.rb.
#
#   brew tap jprodgers/jetcarp
#   brew install jetcarp
#
# (or `brew install jprodgers/jetcarp/jetcarp` in one line).
#
# It installs the release archives the workflow built; nothing is compiled.
class Jetcarp < Formula
  desc "Cellular automata on almost any tiling of the plane"
  homepage "https://jprodgers.github.io/jetcarp/"
  version "@VERSION@"
  license any_of: ["Unlicense", "MIT"]

  on_macos do
    on_arm do
      url "https://github.com/@REPO@/releases/download/v@VERSION@/jetcarp-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA256_AARCH64_APPLE_DARWIN@"
    end
    on_intel do
      url "https://github.com/@REPO@/releases/download/v@VERSION@/jetcarp-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA256_X86_64_APPLE_DARWIN@"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/@REPO@/releases/download/v@VERSION@/jetcarp-@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_AARCH64_UNKNOWN_LINUX_GNU@"
    end
    on_intel do
      url "https://github.com/@REPO@/releases/download/v@VERSION@/jetcarp-@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_X86_64_UNKNOWN_LINUX_GNU@"
    end
  end

  def install
    bin.install "jetcarp"
    # The macOS screen saver's code: `jetcarp host saver install` looks for it in
    # ../lib/jetcarp from the program.
    (lib/"jetcarp").install "libjetcarp_saver.dylib" if OS.mac?
    # The starter pack, when the release has one: found as ../share/jetcarp/packs.
    pkgshare.install "packs" if File.directory?("packs")
    doc.install "README.md", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.html"
    prefix.install "COPYING", "UNLICENSE", "LICENSE-MIT"
  end

  # No post_install step registers the login item with launchd: Homebrew may run it in a sandbox.
  # The program does it itself when the app or the host starts. Nor does one make JETCARP.app or
  # the Linux menu entry, which go in the home folder, out of the sandbox's reach: the caveats
  # name the command.
  def caveats
    <<~EOS
      To open JETCARP from Launchpad, Spotlight and the Dock (macOS, as JETCARP.app in
      ~/Applications) or from your applications menu (Linux), with its icon, run:
        jetcarp launcher install

      Before `brew uninstall jetcarp`, run `jetcarp uninstall` to undo what JETCARP set up
      outside its folders (the background host, login item, screen saver and launcher).

      On macOS, after `brew upgrade jetcarp`: if the desktop background starts at login, open
      the JETCARP app or run `jetcarp host start` once, so that macOS accepts the new version
      at login.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/jetcarp --version")
    ENV["JETCARP_CONFIG_DIR"] = testpath/"config"
    ENV["JETCARP_DATA_DIR"] = testpath/"data"
    ENV["JETCARP_OUT"] = testpath/"out"
    assert_match "settings: #{testpath}/config", shell_output("#{bin}/jetcarp paths")
  end
end
