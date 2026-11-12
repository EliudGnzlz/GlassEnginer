#!/usr/bin/env python3
"""Genera la escena de demostración de render de la Fase 2."""

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "assets" / "scenes" / "Fase2Demo.scene.json"

PALETA_A = [
    [0.82, 0.28, 0.22],
    [0.24, 0.62, 0.86],
    [0.92, 0.72, 0.24],
    [0.34, 0.76, 0.42],
    [0.72, 0.38, 0.82],
]


def crear_entidad(entity_id, nombre, position, rotation=None, scale=None):
    """Copia la plantilla completa de entidad usada por las escenas."""
    return {
        "id": entity_id,
        "name": nombre,
        "tag": "Untagged",
        "layer": "Default",
        "is_static": False,
        "active": True,
        "parent": None,
        "children": [],
        "transform": {
            "position": list(position),
            "rotation": list(rotation or [0, 0, 0]),
            "scale": list(scale or [1, 1, 1]),
        },
        "components": ["Transform"],
        "mesh": None,
        "camera": None,
        "light": None,
        "physics": None,
        "script": None,
        "script_data": None,
        "audio": None,
        "canvas": None,
        "rect_transform": None,
        "ui_text": None,
        "ui_button": None,
        "ui_image": None,
        "ui_drag": None,
        "portal": None,
        "animator": None,
        "fog": None,
    }


def añadir_malla(entidad, color, modelo="cube"):
    entidad["components"].append("MeshRenderer")
    entidad["mesh"] = {"model": modelo, "color": list(color), "wireframe": False, "shader": "pbr.vert.spv"}


def main():
    entidades = []
    siguiente_id = 1

    def añadir(entidad):
        nonlocal siguiente_id
        entidades.append(entidad)
        siguiente_id += 1

    camara = crear_entidad(siguiente_id, "Camara", [0, 3, 14])
    camara["components"] = ["Transform", "Camera"]
    camara["mesh"] = None
    camara["camera"] = {
        "fov": 60,
        "near_clip": 0.1,
        "far_clip": 300,
        "is_main": True,
    }
    camara["fog"] = {
        "color": [0.55, 0.62, 0.72],
        "near": 18.0,
        "far": 46.0,
    }
    añadir(camara)

    sol = crear_entidad(
        siguiente_id, "Sol", [8, 14, 6], rotation=[50, 30, 0]
    )
    sol["components"] = ["Transform", "Light"]
    sol["light"] = {
        "light_type": "Directional",
        "color": [1.0, 0.98, 0.92],
        "intensity": 1.2,
        "range": 0,
        "cast_shadow": False,
    }
    añadir(sol)

    suelo = crear_entidad(
        siguiente_id,
        "Suelo",
        [0, -0.5, 0],
        scale=[90, 1, 90],
    )
    suelo["is_static"] = True
    añadir_malla(suelo, [0.36, 0.38, 0.42])
    suelo["components"].append("Physics")
    suelo["physics"] = {
        "body_type": "Static",
        "mass": 1.0,
        "friction": 0.7,
        "restitution": 0.0,
        "box_extents": [45, 0.5, 45],
        "linear_velocity": [0, 0, 0],
        "layer": 0,
        "collision_mask": 4294967295,
        "is_trigger": False,
        "colliders": [],
        "mesh_collider": "None",
    }
    añadir(suelo)

    # Zona A: visible desde la cámara principal.
    indice = 0
    for z in range(-8, -24, -3):
        for x in range(-13, 14, 3):
            indice += 1
            cubo = crear_entidad(
                siguiente_id,
                f"A_{indice:02d}",
                [x, 0.5, z],
            )
            añadir_malla(cubo, PALETA_A[(indice - 1) % len(PALETA_A)])
            añadir(cubo)

    # Zona B: situada detrás de la cámara.
    indice = 0
    for z in range(22, 44, 3):
        for x in range(-13, 14, 3):
            indice += 1
            cubo = crear_entidad(
                siguiente_id,
                f"B_{indice:02d}",
                [x, 0.5, z],
            )
            añadir_malla(cubo, [0.7, 0.3, 0.3])
            añadir(cubo)

    # Zona C: laterales distantes, fuera del frustum principal.
    indice = 0
    for x in (-34, 34):
        for i in range(35):
            indice += 1
            z = -30.0 + (38.0 * i / 34.0)
            cubo = crear_entidad(
                siguiente_id,
                f"C_{indice:02d}",
                [x, 0.5, z],
            )
            añadir_malla(cubo, [0.3, 0.3, 0.7])
            añadir(cubo)

    posiciones_luces = [
        (-9, 3.2, -11),
        (9, 3.2, -11),
        (-9, 3.2, -20),
        (9, 3.2, -20),
        (0, 2.5, -5),
        (-14, 2.5, -16),
        (14, 2.5, -16),
        (0, 2.5, -26),
    ]
    colores_luces = [
        [1.0, 0.62, 0.30],
        [1.0, 0.78, 0.42],
        [1.0, 0.46, 0.28],
        [1.0, 0.86, 0.58],
        [0.45, 0.68, 1.0],
        [0.58, 1.0, 0.70],
        [0.86, 0.58, 1.0],
        [1.0, 0.88, 0.62],
    ]

    for i, (posicion, color) in enumerate(
        zip(posiciones_luces, colores_luces), start=1
    ):
        con_sombra = i <= 4
        luz = crear_entidad(siguiente_id, f"L{i}", posicion)
        luz["components"] = ["Transform", "Light"]
        luz["light"] = {
            "light_type": "Point",
            "color": color,
            "intensity": 60 if con_sombra else 40,
            "range": 12 if con_sombra else 10,
            "cast_shadow": con_sombra,
        }
        añadir(luz)

    # Dos volúmenes por cada puntual con sombras (L1-L4).
    indice = 0
    for lx, _, lz in posiciones_luces[:4]:
        for desplazamiento in (-0.9, 0.9):
            indice += 1
            flotante = crear_entidad(
                siguiente_id,
                f"Flotante_{indice}",
                [lx + desplazamiento, 1.6, lz],
                scale=[1.2, 1.2, 1.2],
            )
            añadir_malla(
                flotante,
                PALETA_A[(indice - 1) % len(PALETA_A)],
            )
            añadir(flotante)

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT.open("w", encoding="utf-8") as archivo:
        json.dump(entidades, archivo, indent=2, ensure_ascii=False)
        archivo.write("\n")

    total_mallas = sum(e["mesh"] is not None for e in entidades)
    print(f"Escena generada: {OUTPUT}")
    print(f"Total de entidades: {len(entidades)}")
    print(f"Entidades con malla: {total_mallas}")


if __name__ == "__main__":
    main()
