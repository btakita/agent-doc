class AgentDoc < Formula
  desc "Interactive document sessions with AI agents"
  homepage "https://github.com/btakita/agent-doc"
  version "0.25.12"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/btakita/agent-doc/releases/download/v#{version}/agent-doc-v#{version}-macos-x86_64"
      sha256 "FIXME"
    end

    on_arm do
      url "https://github.com/btakita/agent-doc/releases/download/v#{version}/agent-doc-v#{version}-macos-aarch64"
      sha256 "FIXME"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/btakita/agent-doc/releases/download/v#{version}/agent-doc-v#{version}-linux-x86_64"
      sha256 "FIXME"
    end
  end

  def install
    bin.install Dir["agent-doc-v#{version}-*"].first => "agent-doc"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/agent-doc --version")
  end
end
