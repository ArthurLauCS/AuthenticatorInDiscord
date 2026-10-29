# Third-party notices

These notices cover the bundled font and the protocol schema. They do not license AuthTile itself (see `LICENSE`).

## Font: TeX Gyre Heros

`texgyreheros-regular.otf` and `texgyreheros-bold.otf`, version 2.004, in the `fonts` folder next to the exe. Fetched unmodified by `scripts/fonts.ps1` from the GUST e-foundry (<https://www.gust.org.pl/projects/e-foundry/tex-gyre/heros/>, archive `qhv2.004otf.zip`, SHA-256 pinned in the script); not stored in git.

Copyright 2006, 2009 for TeX Gyre extensions by B. Jackowski and J.M. Nowacki (on behalf of TeX users groups). This work is released under the GUST Font License, text below (from <https://www.gust.org.pl/projects/e-foundry/licenses/GUST-FONT-LICENSE.txt>). The font files are distributed unmodified and under their original names. The license refers to the LaTeX Project Public License 1.3c: <https://www.latex-project.org/lppl/lppl-1-3c/>.

```text
% This is a preliminary version (2006-09-30), barring acceptance from
% the LaTeX Project Team and other feedback, of the GUST Font License.
% (GUST is the Polish TeX Users Group, http://www.gust.org.pl)
%
% For the most recent version of this license see
% http://www.gust.org.pl/fonts/licenses/GUST-FONT-LICENSE.txt
% or
% http://tug.org/fonts/licenses/GUST-FONT-LICENSE.txt
%
% This work may be distributed and/or modified under the conditions
% of the LaTeX Project Public License, either version 1.3c of this
% license or (at your option) any later version.
% 
% Please also observe the following clause:
% 1) it is requested, but not legally required, that derived works be
%    distributed only after changing the names of the fonts comprising this
%    work and given in an accompanying "manifest", and that the
%    files comprising the Work, as listed in the manifest, also be given
%    new names. Any exceptions to this request are also given in the
%    manifest.
%    
%    We recommend the manifest be given in a separate file named
%    MANIFEST-<fontid>.txt, where <fontid> is some unique identification
%    of the font family. If a separate "readme" file accompanies the Work, 
%    we recommend a name of the form README-<fontid>.txt.
%
% The latest version of the LaTeX Project Public License is in
% http://www.latex-project.org/lppl.txt and version 1.3c or later
% is part of all distributions of LaTeX version 2006/05/20 or later.
```

Chinese text is drawn with Microsoft YaHei UI (Simplified Chinese UI) or Microsoft JhengHei UI (Traditional Chinese UI), the system fonts that ship with Windows; they are not bundled.

## Protocol schema

`proto/googleauth.proto` is copied unchanged from [JeaNile/gotp](https://github.com/JeaNile/gotp), commit `f49ac7fd5e2445e62f1c6e6e3b449925ba685c2f`, under the MIT license. See `third_party/gotp-LICENSE`.

It documents the Google Authenticator export format (`otpauth-migration://offline?data=…`). `core/src/lib.rs` decodes that format by hand following the field numbers and enum values in this schema; no code is generated from it and no gotp code is used.
