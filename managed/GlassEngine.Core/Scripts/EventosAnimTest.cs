using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Guión de la escena de prueba de eventos de animación
    /// (PruebaEventosAnim.scene.json): se limita a anotar en el log cada
    /// evento que el motor le entrega por OnAnimationEvent, con la
    /// entidad, el nombre y los datos opcionales.
    /// </summary>
    public class EventosAnimTest : GlassBehaviour
    {
        public override void OnAnimationEvent(string name, float? value, string? text)
        {
            Debug.Log($"[AnimEventos] ent={EntityId} evento={name} value={value} text={text}");
        }
    }
}
