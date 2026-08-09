# duallity Coq/Rocq proofs Makefile
#
# Build with resource limiting (recommended for large modular proofs):
#   systemd-run --user --scope -p MemoryMax=8G -p CPUQuota=1800% \
#     -p IOWeight=30 -p TasksMax=200 make -j1
#
# Build without resource limiting:
#   make

COQMAKEFILE = CoqMakefile
COQMAKE = $(MAKE) -f $(COQMAKEFILE)

.PHONY: all clean force proof-check

all: $(COQMAKEFILE)
	$(COQMAKE)

$(COQMAKEFILE): _CoqProject
	coq_makefile -f _CoqProject -o $@

clean:
	if [ -f $(COQMAKEFILE) ]; then $(COQMAKE) clean; fi
	rm -f $(COQMAKEFILE) $(COQMAKEFILE).conf
	rm -f *.glob *.vo *.vok *.vos *.aux .*.aux

force:
	touch _CoqProject
	$(MAKE) all

# Reject unchecked or vacuous proof escapes (CI + verify.sh gate).
proof-check:
	@echo "Checking for unchecked proof escapes..."
	@if grep -REn "^[[:space:]]*(Admitted|Abort)\.|^[[:space:]]*(Axiom|Conjecture|Parameter)[[:space:]]|\badmit\.|True[[:space:]]*\(\*|True\." *.v 2>/dev/null; then \
		echo "ERROR: Found unchecked or vacuous proof escapes."; \
		exit 1; \
	else \
		echo "No unchecked proof escapes found."; \
	fi
