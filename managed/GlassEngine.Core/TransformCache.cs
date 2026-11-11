using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

/// <summary>
/// Caché de transforms del frame (bloque «FFI por lotes»): al
/// abrir el tick se trae TODA la escena en un solo cruce FFI;
/// durante el tick, <c>Transform</c> lee y escribe aquí en memoria
/// gestionada; al cerrar, los transforms tocados vuelven a Rust en
/// otro único cruce. Fuera del tick (o sin el puente de lotes) la
/// caché está inactiva y <c>Transform</c> usa la ruta FFI clásica
/// por entidad.
/// </summary>
internal static class TransformCache
{
    private static ulong[] _ids = new ulong[1];
    private static TransformData[] _data = new TransformData[1];
    private static readonly Dictionary<ulong, int> _indice = new();
    private static readonly HashSet<int> _sucios = new();
    private static ulong[] _idsFlush = new ulong[1];
    private static TransformData[] _dataFlush = new TransformData[1];
    private static int _count;

    public static bool Active { get; private set; }

    public static void BeginFrame()
    {
        Active = false;
        _count = 0;
        _indice.Clear();
        _sucios.Clear();

        if (!RuntimeBridge.TransformBatchAvailable)
        {
            return;
        }

        int esperados = RuntimeBridge.GetTransformsBatch(IntPtr.Zero, IntPtr.Zero, 0);
        if (esperados < 0)
        {
            return;
        }

        AsegurarCapacidad(esperados);

        GCHandle idsFijados = default;
        GCHandle dataFijados = default;

        try
        {
            idsFijados = GCHandle.Alloc(_ids, GCHandleType.Pinned);
            dataFijados = GCHandle.Alloc(_data, GCHandleType.Pinned);

            int recibidos = RuntimeBridge.GetTransformsBatch(
                idsFijados.AddrOfPinnedObject(),
                dataFijados.AddrOfPinnedObject(),
                _ids.Length);

            if (recibidos < esperados || recibidos < 0 || recibidos > _ids.Length)
            {
                return;
            }

            _count = recibidos;
            for (int i = 0; i < _count; i++)
            {
                _indice[_ids[i]] = i;
            }

            Active = true;
        }
        finally
        {
            if (dataFijados.IsAllocated)
            {
                dataFijados.Free();
            }

            if (idsFijados.IsAllocated)
            {
                idsFijados.Free();
            }
        }
    }

    public static bool TryRead(ulong id, out TransformData t)
    {
        if (Active && _indice.TryGetValue(id, out int indice) && indice < _count)
        {
            t = _data[indice];
            return true;
        }

        t = default;
        return false;
    }

    public static bool Write(ulong id, TransformData t)
    {
        if (!Active || !_indice.TryGetValue(id, out int indice) || indice >= _count)
        {
            return false;
        }

        _data[indice] = t;
        _sucios.Add(indice);
        return true;
    }

    public static void Flush()
    {
        try
        {
            if (!Active || _sucios.Count == 0)
            {
                return;
            }

            AsegurarCapacidadFlush(_sucios.Count);

            int cantidad = 0;
            foreach (int indice in _sucios)
            {
                if (indice < 0 || indice >= _count)
                {
                    continue;
                }

                _idsFlush[cantidad] = _ids[indice];
                _dataFlush[cantidad] = _data[indice];
                cantidad++;
            }

            if (cantidad == 0)
            {
                return;
            }

            GCHandle idsFijados = default;
            GCHandle dataFijados = default;

            try
            {
                idsFijados = GCHandle.Alloc(_idsFlush, GCHandleType.Pinned);
                dataFijados = GCHandle.Alloc(_dataFlush, GCHandleType.Pinned);

                RuntimeBridge.SetTransformsBatch(
                    idsFijados.AddrOfPinnedObject(),
                    dataFijados.AddrOfPinnedObject(),
                    cantidad);
            }
            finally
            {
                if (dataFijados.IsAllocated)
                {
                    dataFijados.Free();
                }

                if (idsFijados.IsAllocated)
                {
                    idsFijados.Free();
                }
            }
        }
        finally
        {
            _sucios.Clear();
            Active = false;
        }
    }

    public static void EndFrame()
    {
        Active = false;
    }

    private static void AsegurarCapacidad(int requerida)
    {
        if (_ids.Length >= requerida)
        {
            return;
        }

        int capacidad = _ids.Length;
        while (capacidad < requerida)
        {
            capacidad *= 2;
        }

        Array.Resize(ref _ids, capacidad);
        Array.Resize(ref _data, capacidad);
    }

    private static void AsegurarCapacidadFlush(int requerida)
    {
        if (_idsFlush.Length >= requerida)
        {
            return;
        }

        int capacidad = _idsFlush.Length;
        while (capacidad < requerida)
        {
            capacidad *= 2;
        }

        Array.Resize(ref _idsFlush, capacidad);
        Array.Resize(ref _dataFlush, capacidad);
    }
}
