using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// La barca del nivel 2: una plataforma móvil ancha que cruza el lago
    /// de orilla a orilla (recorrido ±5,6 desde su ancla) y espera 1,6 s
    /// en cada extremo para subir/bajar. El jugador se monta y es
    /// transportado por el mismo mecanismo de deltas que las plataformas
    /// (su raycast de suelo detecta la barca y aplica el desplazamiento).
    ///
    /// Estela: la v2 soltaba un anillo de espuma expansivo en la popa
    /// cada 0,65 m; el usuario ordenó retirar los anillos (a su ojo se
    /// leían como líneas/parches que la barca «va dejando», y una
    /// estela real se encoge y se desvanece de forma continua, cosa
    /// que unos anillos estampados no logran). Desde entonces la barca
    /// no pide ningún anillo: el agua reacciona solo con la huella de
    /// interacción (espuma de contacto rectangular). Los Plat3DWakeRing
    /// de la escena quedan dormidos en su pool, sin activarse nunca.
    /// </summary>
    public class Plat3DBoat : Plat3DMovingPlatform
    {
        protected override float Range => 5.6f;
        protected override float Speed => 1.7f;
        protected override float Dwell => 1.6f;

        // Huella del casco para la interacción de los grafos: su caja en
        // planta, la de su propio collider (box_extents 2,1 x 1,6 m).
        // El casco mide 4,2 x 3,2 m; publicar un disco de radio fijo
        // dibujaba un círculo alrededor de un objeto rectangular — en
        // las esquinas sobraba efecto y en los lados largos faltaba —,
        // así que el agua ahora abraza el rectángulo del casco.
        private const float HullHalfX = 2.1f;
        private const float HullHalfZ = 1.6f;

        public override void Update(float deltaTime)
        {
            base.Update(deltaTime);

            // Interacción de los grafos: la barca publica su huella
            // cada frame para que el agua reaccione a su paso
            // (depresión bajo el casco, anillo levantado y espuma de
            // contacto en el grafo agua). La huella es la CAJA de su
            // collider en planta, con su rotación Y: la barca se mueve
            // en ping-pong sobre X sin girar nunca (la rotación que
            // captura Start es la suya para siempre), así que el yaw
            // publicado es constante — pero sale de su rotación real,
            // no de una constante: si un día la barca girara al dar la
            // vuelta, la huella giraría con ella sin tocar este código.
            // Se publica también en espera en la orilla: el agua debe
            // abrazar el casco aunque la barca no se mueva. La Y publicada
            // es la de la SUPERFICIE del agua cuando la barca está
            // dentro de una zona; fuera de zonas, su propia Y. Al
            // cambiar de escena el motor limpia el punto solo (esta
            // barca deja de existir con la escena).
            Vector3 boatPos = Position;
            float publishY = boatPos.Y;
            for (int i = 0; i < Plat3DGameState.WaterZones.Count; i++)
            {
                Plat3DWaterZoneData zone = Plat3DGameState.WaterZones[i];
                if (zone.ContainsXZ(boatPos))
                {
                    publishY = zone.SurfaceY;
                    break;
                }
            }
            Quaternion q = Rotation;
            // El yaw sale del vector forward del cuaternión
            // (q * (0,0,1)): yaw = atan2(f.x, f.z), la misma convención
            // con la que el nodo de grafo deshace la rotación en el
            // shader.
            float forwardX = 2f * (q.X * q.Z + q.W * q.Y);
            float forwardZ = 1f - 2f * (q.X * q.X + q.Y * q.Y);
            float yaw = MathF.Atan2(forwardX, forwardZ);
            UnityEngine.Shader.SetInteractionBox(
                new UnityEngine.Vector3(boatPos.X, publishY, boatPos.Z),
                HullHalfX, HullHalfZ, yaw);

            // Sin control de estela: la barca ya no pide anillos
            // (retirados por decisión del usuario). Su única huella
            // sobre el agua es la de interacción publicada arriba.
        }
    }
}
