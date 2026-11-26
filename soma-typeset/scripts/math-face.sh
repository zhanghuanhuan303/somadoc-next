#!/bin/sh
# Print the first installed math face, in the reference order.
#
# New Computer Modern Math is the embedded table's face and Typst's default
# (ADR-0079), so it is preferred for samples; Latin Modern, DejaVu, Libertinus
# and Noto are fallbacks. Scripts use this as the default of SOMA_SONG_MATH /
# SOMA_PDF_MATH. Prints nothing when no face is installed; the caller decides
# whether that is an error.
for candidate in \
    /usr/share/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf \
    /usr/share/texmf/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf \
    /usr/share/texlive/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf \
    /usr/share/fonts/OTF/NewCMMath-Regular.otf \
    /usr/share/fonts/newcomputermodern/NewCMMath-Regular.otf \
    /usr/share/texmf-dist/fonts/opentype/public/lm-math/latinmodern-math.otf \
    /usr/share/texmf/fonts/opentype/public/lm-math/latinmodern-math.otf \
    /usr/share/texlive/texmf-dist/fonts/opentype/public/lm-math/latinmodern-math.otf \
    /usr/share/fonts/OTF/latinmodern-math.otf \
    /usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf \
    /usr/share/fonts/libertinus/LibertinusMath-Regular.ttf \
    /usr/share/fonts/noto/NotoSansMath-Regular.ttf; do
    if [ -f "$candidate" ]; then
        printf '%s\n' "$candidate"
        break
    fi
done
exit 0
