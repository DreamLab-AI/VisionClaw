// Run: nvcc -std=c++17 tests/gpu/integrate_bounds.cu -o target/vc-integrate-bounds && target/vc-integrate-bounds
#include "../../crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu"
#include <cstdio>
#include <cstdlib>
#define CUDA_OK(call) do { auto e = (call); if (e != cudaSuccess) { fprintf(stderr, "%s\n", cudaGetErrorString(e)); return 2; } } while(0)
int main() {
    float *p, *v, *zero, *out, *vout;
    int *pin;
    for (float** ptr : {&p, &v, &zero, &out, &vout}) CUDA_OK(cudaMallocManaged(ptr, 9 * sizeof(float)));
    CUDA_OK(cudaMallocManaged(&pin, 3 * sizeof(int)));
    for (int a=0; a<3; a++) { p[a*3] = 9; p[a*3+1] = -9; p[a*3+2] = 20; v[a*3] = 50; v[a*3+1] = -50; v[a*3+2] = 0; }
    for (int i=0; i<9; i++) zero[i] = 0;
    for (int i=0; i<3; i++) pin[i] = i == 2;
    SimParams params{};
    params.dt = 1; params.damping = 1; params.max_velocity = 1000;
    params.viewport_bounds = 10; params.boundary_damping = 1;
    CUDA_OK(cudaMemcpyToSymbol(c_params, &params, sizeof(params)));
    integrate_pass_kernel<<<1,32>>>(p,p+3,p+6,v,v+3,v+6,zero,zero+3,zero+6,nullptr,
        out,out+3,out+6,vout,vout+3,vout+6,3,nullptr,nullptr,nullptr,nullptr,nullptr,nullptr,pin);
    CUDA_OK(cudaGetLastError()); CUDA_OK(cudaDeviceSynchronize());
    for (int i=0; i<9; i++) printf("node-axis%d position=%g velocity=%g\n", i, out[i], vout[i]);
    for (int a=0; a<3; a++) if (out[a*3] != 10 || out[a*3+1] != -10 || out[a*3+2] != 20 || vout[a*3] != 0 || vout[a*3+1] != 0 || vout[a*3+2] != 0) return 1;
    printf("PASS: positive/negative free-node overshoot clamped; outward velocity stopped; pinned position preserved\n");
    float *degree;
    AABB *all, *connected;
    CUDA_OK(cudaMallocManaged(&degree, 3 * sizeof(float)));
    CUDA_OK(cudaMallocManaged(&all, sizeof(AABB)));
    CUDA_OK(cudaMallocManaged(&connected, sizeof(AABB)));
    degree[0] = 1; degree[1] = 2; degree[2] = 0;
    for (int axis=0; axis<3; axis++) { p[axis*3] = -2; p[axis*3+1] = 3; p[axis*3+2] = 10000; }
    compute_aabb_reduction_kernel<<<1,32,6*32*sizeof(float)>>>(p,p+3,p+6,all,3);
    compute_connected_aabb_reduction_kernel<<<1,32,6*32*sizeof(float)>>>(p,p+3,p+6,degree,connected,3);
    CUDA_OK(cudaGetLastError()); CUDA_OK(cudaDeviceSynchronize());
    if (all->max.x != 10000 || connected->min.x != -2 || connected->max.x != 3 || connected->max.y != 3 || connected->max.z != 3) return 1;
    degree[0] = 0; degree[1] = 0;
    compute_connected_aabb_reduction_kernel<<<1,32,6*32*sizeof(float)>>>(p,p+3,p+6,degree,connected,3);
    CUDA_OK(cudaGetLastError()); CUDA_OK(cudaDeviceSynchronize());
    if (!(connected->min.x > connected->max.x)) return 1;
    printf("PASS: connected extent excludes isolated outlier; spatial extent retains it; empty population is invalid extent\n");
    CUDA_OK(cudaFree(degree)); CUDA_OK(cudaFree(all)); CUDA_OK(cudaFree(connected));
    for (float* ptr : {p,v,zero,out,vout}) CUDA_OK(cudaFree(ptr));
    CUDA_OK(cudaFree(pin));
}
