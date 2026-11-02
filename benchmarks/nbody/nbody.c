/* nbody: the same algorithm as nbody.aipl (Benchmarks Game n-body), in C. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

static double x[5], y[5], z[5], vx[5], vy[5], vz[5], m[5];
static void body(int i, double a, double b, double c, double d, double e, double f, double g) {
    x[i] = a; y[i] = b; z[i] = c; vx[i] = d; vy[i] = e; vz[i] = f; m[i] = g;
}
static double energy(void) {
    double e = 0.0;
    for (int i = 0; i < 5; i++) {
        e = e + (0.5 * m[i]) * ((vx[i] * vx[i] + vy[i] * vy[i]) + vz[i] * vz[i]);
        for (int j = i + 1; j < 5; j++) {
            double dx = x[i] - x[j], dy = y[i] - y[j], dz = z[i] - z[j];
            double distance = sqrt((dx * dx + dy * dy) + dz * dz);
            e = e - (m[i] * m[j]) / distance;
        }
    }
    return e;
}
static void advance(double dt) {
    for (int i = 0; i < 5; i++)
        for (int j = i + 1; j < 5; j++) {
            double dx = x[i] - x[j], dy = y[i] - y[j], dz = z[i] - z[j];
            double dsq = (dx * dx + dy * dy) + dz * dz;
            double distance = sqrt(dsq);
            double mag = dt / (dsq * distance);
            double mi = m[i], mj = m[j];
            vx[i] = vx[i] - (dx * mj) * mag; vy[i] = vy[i] - (dy * mj) * mag; vz[i] = vz[i] - (dz * mj) * mag;
            vx[j] = vx[j] + (dx * mi) * mag; vy[j] = vy[j] + (dy * mi) * mag; vz[j] = vz[j] + (dz * mi) * mag;
        }
    for (int i = 0; i < 5; i++) { x[i] = x[i] + dt * vx[i]; y[i] = y[i] + dt * vy[i]; z[i] = z[i] + dt * vz[i]; }
}
int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 1000;
    double pi = 3.141592653589793, SM = (4.0 * pi) * pi, DPY = 365.24;
    body(0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SM);
    body(1, 4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03 * DPY, 7.69901118419740425e-03 * DPY, -6.90460016972063023e-05 * DPY, 9.54791938424326609e-04 * SM); /* jupiter */
    body(2, 8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03 * DPY, 4.99852801234917238e-03 * DPY, 2.30417297573763929e-05 * DPY, 2.85885980666130812e-04 * SM); /* saturn */
    body(3, 1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03 * DPY, 2.37847173959480950e-03 * DPY, -2.96589568540237556e-05 * DPY, 4.36624404335156298e-05 * SM); /* uranus */
    body(4, 1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03 * DPY, 1.62824170038242295e-03 * DPY, -9.51592254519715870e-05 * DPY, 5.15138902046611451e-05 * SM); /* neptune */
    double px = 0.0, py = 0.0, pz = 0.0;
    for (int i = 0; i < 5; i++) { px = px + vx[i] * m[i]; py = py + vy[i] * m[i]; pz = pz + vz[i] * m[i]; }
    vx[0] = (0.0 - px) / SM; vy[0] = (0.0 - py) / SM; vz[0] = (0.0 - pz) / SM;
    printf("%.9f\n", energy());
    for (int s = 0; s < n; s++) advance(0.01);
    printf("%.9f\n", energy());
    return 0;
}
