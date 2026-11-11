"""Genera escenas de estrés con cuerpos dinámicos dispuestos en rejilla."""

import json
import math
from pathlib import Path


RAIZ_PROYECTO = Path(__file__).resolve().parents[1]
DIRECTORIO_ESCENAS = RAIZ_PROYECTO / "assets" / "scenes"


def entidad_base(identificador, nombre, position, rotation=None, scale=None):
    """Crea los campos comunes de una entidad."""
    return {
        "id": identificador,
        "active": True,
        "name": nombre,
        "tag": "Untagged",
        "layer": "Default",
        "is_static": False,
        "components": [],
        "transform": {
            "position": position,
            "rotation": rotation or [0, 0, 0],
            "scale": scale or [1, 1, 1],
        },
        "mesh": None,
        "script": None,
        "script_data": None,
        "physics": None,
        "audio": None,
        "camera": None,
        "light": None,
        "canvas": None,
        "rect_transform": None,
        "ui_text": None,
        "ui_button": None,
        "parent": None,
        "children": [],
    }


def crear_mesh(modelo, color):
    return {
        "color": color,
        "wireframe": False,
        "shader": "pbr.vert.spv",
        "model": modelo,
    }


def crear_fisica(tipo, semiejes, es_trigger=False):
    return {
        "body_type": tipo,
        "mass": 1.0,
        "friction": 0.7,
        "restitution": 0.0,
        "box_extents": semiejes,
        "linear_velocity": [0, 0, 0],
        "layer": 0,
        "collision_mask": 4294967295,
        "is_trigger": es_trigger,
    }


def crear_escena(cantidad):
    entidades = []

    camara = entidad_base(1, "Cámara", [0, 26, 30], [-50, 0, 0])
    camara["components"] = ["Transform", "Camera"]
    camara["camera"] = {
        "fov": 60.0,
        "near_clip": 0.1,
        "far_clip": 1000.0,
        "is_main": True,
    }
    entidades.append(camara)

    luz = entidad_base(2, "Luz direccional", [0, 0, 0], [-58, -22, 0])
    luz["components"] = ["Transform", "Light"]
    luz["light"] = {
        "light_type": "Directional",
        "color": [1, 1, 1],
        "intensity": 1.0,
        "range": 10.0,
    }
    entidades.append(luz)

    suelo = entidad_base(
        3, "Suelo", [0, -0.5, 0], scale=[60, 1, 60]
    )
    suelo["is_static"] = True
    suelo["components"] = ["Transform", "MeshRenderer", "StaticCollider"]
    suelo["mesh"] = crear_mesh("cube", [0.25, 0.27, 0.3])
    suelo["physics"] = crear_fisica("Static", [30, 0.5, 30])
    entidades.append(suelo)

    lado = math.ceil(math.sqrt(cantidad))
    separacion = 1.6
    desplazamiento = (lado - 1) * separacion / 2

    for indice in range(cantidad):
        fila, columna = divmod(indice, lado)
        es_esfera = indice % 2 == 0
        cuerpo = entidad_base(
            indice + 4,
            f"Cuerpo {indice + 1:03d}",
            [
                columna * separacion - desplazamiento,
                3 + (fila % 5) * 1.3,
                fila * separacion - desplazamiento,
            ],
        )
        cuerpo["components"] = [
            "Transform", "MeshRenderer", "RigidBody", "Collider"
        ]
        cuerpo["mesh"] = crear_mesh(
            "sphere" if es_esfera else "cube",
            [0.9, 0.35, 0.2] if es_esfera else [0.2, 0.55, 0.9],
        )
        cuerpo["physics"] = crear_fisica("Dynamic", [0.5, 0.5, 0.5])
        entidades.append(cuerpo)

    return entidades


def main():
    DIRECTORIO_ESCENAS.mkdir(parents=True, exist_ok=True)
    for cantidad in (100, 500, 1000):
        entidades = crear_escena(cantidad)
        ruta = DIRECTORIO_ESCENAS / f"StressTest{cantidad}.json"
        with ruta.open("w", encoding="utf-8") as archivo:
            json.dump(entidades, archivo, ensure_ascii=False, indent=2)
            archivo.write("\n")
        print(f"{ruta.name}: {len(entidades)} entidades escritas")


if __name__ == "__main__":
    main()
