using UnityEngine;

/// <summary>
/// Sonda C3: Get/AddComponent por clase de forma y OnControllerColliderHit
/// con el tipo de fachada.
/// </summary>
public class SondaFormas : MonoBehaviour
{
    public override void Start()
    {
        // 1. Alta de una esfera y ajuste del radio.
        var esfera = gameObject.AddComponent<SphereCollider>();
        esfera.radius = 0.75f;
        Debug.Log("[SONDA-F] AddComponent<SphereCollider> radio=" + esfera.radius);

        // 2. Recuperación por su clase concreta.
        var otraEsfera = GetComponent<SphereCollider>();
        Debug.Log("[SONDA-F] GetComponent<SphereCollider> misma instancia="
                  + object.ReferenceEquals(otraEsfera, esfera)
                  + " radio=" + (otraEsfera == null ? "null" : otraEsfera.radius.ToString()));

        // 3. Recuperación por la base: debe devolver la primera forma (SphereCollider).
        var baseCollider = GetComponent<Collider>();
        Debug.Log("[SONDA-F] GetComponent<Collider> tipo="
                  + (baseCollider == null ? "null" : baseCollider.GetType().Name));

        // 4. Cápsula con sus defectos.
        var capsula = gameObject.AddComponent<CapsuleCollider>();
        Debug.Log("[SONDA-F] CapsuleCollider altura=" + capsula.height
                  + " direccion=" + capsula.direction);

        // 5. Forma no añadida: debe ser null.
        var caja = GetComponent<BoxCollider>();
        Debug.Log("[SONDA-F] BoxCollider: " + (caja == null ? "null" : caja.GetType().Name));

        // 6. Malla convexa.
        var malla = gameObject.AddComponent<MeshCollider>();
        malla.convex = true;
        Debug.Log("[SONDA-F] MeshCollider convex=" + GetComponent<MeshCollider>()?.convex);
    }

    // 7. En esta escena no hay personaje, así que no debería llegar nunca.
    // Su compilación con el tipo de fachada ya forma parte de la prueba.
    void OnControllerColliderHit(ControllerColliderHit hit)
    {
        Debug.Log("[SONDA-F] hit de controlador recibido");
    }
}
