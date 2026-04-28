PAPER := main
LATEXMK := latexmk
BENCH_DIR := benchmarks/c1

.PHONY: all clean distclean benchmarks watch anon

all: $(PAPER).pdf

$(PAPER).pdf: $(PAPER).tex sections/*.tex tables/*.tex figures/*.tex references.bib
	$(LATEXMK) -pdf -interaction=nonstopmode -halt-on-error $(PAPER).tex

watch:
	$(LATEXMK) -pdf -pvc -interaction=nonstopmode $(PAPER).tex

anon:
	@echo "Set \\anonymoustrue in main.tex before submission build."
	$(LATEXMK) -pdf -interaction=nonstopmode -halt-on-error $(PAPER).tex

clean:
	$(LATEXMK) -c
	rm -f *.bbl *.run.xml

distclean:
	$(LATEXMK) -C
	rm -f *.bbl *.run.xml

benchmarks:
	@echo "Running C1 (Stellar+BLS12-381+Groth16) benchmark harness..."
	bash $(BENCH_DIR)/run.sh
	@echo "Results written to $(BENCH_DIR)/results.json"
	@echo "Re-run 'make' to regenerate PDF with updated table cells."
