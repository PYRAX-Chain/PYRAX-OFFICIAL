import { Github, Globe } from "lucide-react";

export function Footer() {
  const currentYear = new Date().getFullYear();

  return (
    <footer className="border-t border-border/50 bg-gradient-to-t from-background to-transparent py-8 mt-12">
      <div className="max-w-7xl mx-auto px-4 md:px-8">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-8 mb-8">
          {/* Brand */}
          <div className="space-y-2">
            <h3 className="font-semibold pyrax-gradient-text text-lg">PYRAX</h3>
            <p className="text-sm text-muted-foreground">
              Decentralized blockchain with integrated AI/ML compute marketplace.
            </p>
          </div>

          {/* Links */}
          <div className="space-y-2">
            <h4 className="font-semibold text-sm text-foreground">Resources</h4>
            <ul className="space-y-1 text-sm">
              <li>
                <a 
                  href="https://pyrax.org" 
                  target="_blank" 
                  rel="noopener noreferrer"
                  className="text-muted-foreground hover:text-orange-400 transition-colors"
                >
                  Website
                </a>
              </li>
              <li>
                <a 
                  href="https://github.com/pyrax-official/pyrax" 
                  target="_blank" 
                  rel="noopener noreferrer"
                  className="text-muted-foreground hover:text-orange-400 transition-colors"
                >
                  GitHub
                </a>
              </li>
              <li>
                <a 
                  href="https://docs.pyrax.org" 
                  target="_blank" 
                  rel="noopener noreferrer"
                  className="text-muted-foreground hover:text-orange-400 transition-colors"
                >
                  Documentation
                </a>
              </li>
            </ul>
          </div>

          {/* Status */}
          <div className="space-y-2">
            <h4 className="font-semibold text-sm text-foreground">Status</h4>
            <p className="text-sm text-muted-foreground">
              RPC: <span className="text-orange-400 font-mono">rpc.pyrax-devnet.org</span>
            </p>
            <p className="text-sm text-muted-foreground">
              Metrics: <span className="text-orange-400 font-mono">localhost:8080</span>
            </p>
          </div>
        </div>

        {/* Divider */}
        <div className="border-t border-border/30 pt-6">
          <div className="flex flex-col md:flex-row justify-between items-center gap-4 text-xs text-muted-foreground">
            <p>© {currentYear} PYRAX. All rights reserved.</p>
            <div className="flex items-center gap-4">
              <a 
                href="https://pyrax.org" 
                target="_blank" 
                rel="noopener noreferrer"
                className="hover:text-orange-400 transition-colors"
                title="Visit pyrax.org"
              >
                <Globe className="w-4 h-4" />
              </a>
              <a 
                href="https://github.com/pyrax-official/pyrax" 
                target="_blank" 
                rel="noopener noreferrer"
                className="hover:text-orange-400 transition-colors"
                title="Visit GitHub"
              >
                <Github className="w-4 h-4" />
              </a>
            </div>
          </div>
        </div>
      </div>
    </footer>
  );
}
