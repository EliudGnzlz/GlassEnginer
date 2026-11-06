using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Guión del objeto arrastrable de InventoryTest: anota el
    /// inicio y el fin del arrastre (con la casilla destino que
    /// publica el motor en DragDrop).
    /// </summary>
    public class InventoryItemLogger : GlassBehaviour
    {
        public void OnItemBegin()
        {
            Debug.Log($"[Inventory] BEGIN drag del objeto entidad={EntityId}");
        }

        public void OnItemEnd()
        {
            Debug.Log($"[Inventory] END drag del objeto entidad={EntityId}; destino={DragDrop.DropTargetEntityId}");
        }
    }

    /// <summary>
    /// Guión de cada casilla de InventoryTest: anota la suelta
    /// recibida (qué objeto cayó sobre ella).
    /// </summary>
    public class InventorySlotLogger : GlassBehaviour
    {
        public void OnSlotDrop()
        {
            Debug.Log($"[Inventory] DROP en la casilla entidad={EntityId}; objeto={DragDrop.DraggedEntityId}");
        }
    }
}
