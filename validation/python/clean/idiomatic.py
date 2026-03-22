# Expected: exit 0
# Expected: sections=imports,classes,fns
# Expected: contains=ConfigLoader
# Expected: contains=load
# Expected: contains=save

import json
from pathlib import Path
from typing import Optional, Dict


class ConfigLoader:
    """Manages application configuration."""

    def __init__(self, path: str):
        self.path = Path(path)

    def load(self) -> Optional[Dict]:
        """Load config from disk."""
        if not self.path.exists():
            return None
        with open(self.path) as f:
            return json.load(f)

    def save(self, data: Dict) -> None:
        """Save config to disk."""
        with open(self.path, "w") as f:
            json.dump(data, f, indent=2)


def create_default_config() -> Dict:
    """Create a default configuration."""
    return {"version": 1, "debug": False}
