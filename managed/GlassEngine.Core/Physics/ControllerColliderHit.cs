using System;
using UnityEngine;

namespace GlassEngine.Core.Physics
{
    [Flags]
    public enum CollisionFlags
    {
        None = 0,
        Below = 1,
        Sides = 2,
        Above = 4,
    }

    public class ControllerColliderHit
    {
        public Vector3 point;
        public Vector3 normal;
        public GameObject gameObject;
        public Vector3 moveDirection;
        public float moveLength;

        internal static ControllerColliderHit FromFfi(in GlassEngine.Core.Interop.CharacterHitFfi h)
        {
            return new ControllerColliderHit
            {
                point = new Vector3(h.PointX, h.PointY, h.PointZ),
                normal = new Vector3(h.NormalX, h.NormalY, h.NormalZ),
                gameObject = GameObject.GetOrCreate(h.Entity, "Entity"),
                moveDirection = new Vector3(h.MoveDirX, h.MoveDirY, h.MoveDirZ),
                moveLength = h.MoveLength,
            };
        }
    }
}
