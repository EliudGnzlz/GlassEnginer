using System;
using UnityEngine;
using Input = UnityEngine.Input;

namespace GlassEngine.Core.Scripts
{
    public class UnityPlayerController : MonoBehaviour
    {
        public float speed = 5.0f;
        public float jumpForce = 5.0f;
        private Rigidbody? _rb;

        public override void Start()
        {
            _rb = GetComponent<Rigidbody>();
            Debug.Log($"[UnityPlayerController] Inicializado en {gameObject.name} con velocidad {speed}");
        }

        public override void Update()
        {
            float h = Input.GetAxis("Horizontal");
            float v = Input.GetAxis("Vertical");
            Vector3 move = new Vector3(h, 0, v) * (speed * Time.deltaTime);
            transform.Translate(move);

            if (Input.GetKey(KeyCode.Space) && _rb != null)
            {
                _rb.AddImpulse(Vector3.up * jumpForce);
            }
        }
    }
}
