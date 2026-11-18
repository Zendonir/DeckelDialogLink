"""Allow ``python -m deckellink``."""

import sys

from deckellink.app import main

if __name__ == "__main__":
    sys.exit(main())
