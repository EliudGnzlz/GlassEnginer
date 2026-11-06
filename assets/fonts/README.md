# Fuentes del motor

## DejaVuSans.ttf

Fuente por defecto de la UI de juego en tiempo de ejecución (Canvas del
standalone). Se incrusta en el binario del renderer (`include_bytes!`) y se
rasteriza en tiempo de arranque a un atlas de glifos en VRAM
(`crates/glass_renderer/src/ui.rs`); no se lee del disco en runtime.

- Origen: proyecto DejaVu Fonts (https://dejavu-fonts.github.io/).
- Licencia: licencia propia de DejaVu, de tipo permisivo (uso, copia,
  modificación y redistribución libres, con la condición de no vender la
  fuente por sí sola y de conservar el aviso de copyright). Es la fuente
  que distribuyen por defecto la mayoría de distribuciones Linux
  (`/usr/share/fonts/truetype/dejavu/`), de donde se copió este fichero.
