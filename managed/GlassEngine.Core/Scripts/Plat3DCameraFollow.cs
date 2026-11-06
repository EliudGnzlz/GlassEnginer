using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Audio;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Cámara del plataformas 3D, con dos modos que se alternan con la
    /// tecla V (borde detectado aquí; el motor no tiene GetKeyDown para
    /// teclas sueltas):
    ///  - Tercera persona: sigue al jugador desde (0, +7,2, +10,2) con
    ///    cabeceo fijo de -34°, suavizada con lerp.
    ///  - Primera persona: cámara en la cabeza del jugador (+0,62 y
    ///    0,30 hacia su frente), mirando a su frente con cabeceo de -3°.
    ///    El cuerpo del jugador se oculta poniendo su escala de escena a
    ///    0,001 (el sync de física no escribe la escala, así que aguanta;
    ///    al volver a 3ª persona se restaura a 1). El giro en 1ª persona
    ///    es con A/D y lo hace el controlador del jugador, no hay
    ///    mouse-look porque el motor no expone captura de puntero.
    /// </summary>
    public class Plat3DCameraFollow : GlassBehaviour
    {
        private bool _prevV;
        private bool _snapped;

        private static readonly Vector3 ThirdPersonOffset =
            new Vector3 { X = 0f, Y = 7.2f, Z = 10.2f };

        public override void Update(float deltaTime)
        {
            if (!Plat3DGameState.PlayerPositionValid)
            {
                return;
            }

            // --- Tecla V: cambio de modo ---
            bool vNow = Input.GetKey("v");
            if (vNow && !_prevV)
            {
                Plat3DGameState.FirstPerson = !Plat3DGameState.FirstPerson;
                AudioSource.Play2D("plat3d_click", 0.6f, 1.0f, false);
                SetPlayerBodyVisible(!Plat3DGameState.FirstPerson);
                _snapped = false;
            }
            _prevV = vNow;

            Vector3 playerPos = Plat3DGameState.PlayerPosition;

            if (Plat3DGameState.FirstPerson)
            {
                Vector3 facing = Plat3DGameState.PlayerFacing;
                Vector3 head = new Vector3
                {
                    X = playerPos.X + facing.X * 0.30f,
                    Y = playerPos.Y + 0.62f,
                    Z = playerPos.Z + facing.Z * 0.30f
                };
                Position = head;
                Quaternion yaw = Plat3DMath.YawQuaternion(Plat3DGameState.PlayerFacingYaw);
                Quaternion pitch = Plat3DMath.PitchQuaternion(-3f);
                Rotation = Plat3DMath.Multiply(yaw, pitch);
            }
            else
            {
                Vector3 target = playerPos + ThirdPersonOffset;
                if (!_snapped)
                {
                    Position = target;
                    _snapped = true;
                }
                else
                {
                    float k = MathF.Min(1f, deltaTime * 6f);
                    Position = Plat3DMath.Lerp(Position, target, k);
                }
                Rotation = Plat3DMath.PitchQuaternion(-34f);
            }
        }

        private void SetPlayerBodyVisible(bool visible)
        {
            Plat3DPlayerController player = Plat3DGameState.Player;
            if (player == null)
            {
                return;
            }
            if (RuntimeBridge.GetTransform(player.EntityId, out TransformData t) == 0)
            {
                float s = visible ? 1f : 0.001f;
                t.Scale = new Vector3 { X = s, Y = s, Z = s };
                RuntimeBridge.SetTransform(player.EntityId, ref t);
            }
        }
    }
}
