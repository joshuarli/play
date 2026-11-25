import os
import sys

print(os.stat(sys.argv[1]).st_gid)
