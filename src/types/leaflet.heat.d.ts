declare module "leaflet.heat";

declare namespace L {
  export interface HeatLatLngTuple extends Array<number> {
    0: number;
    1: number;
    2?: number;
    length: 3;
  }

  export class HeatLayer extends Layer {
    constructor(
      latlngs: Array<[number, number, number?]>,
      options?: {
        minOpacity?: number;
        maxZoom?: number;
        max?: number;
        radius?: number;
        blur?: number;
        gradient?: Record<number, string>;
      },
    );
    setLatLngs(latlngs: Array<[number, number, number?]>): this;
    addLatLng(latlng: [number, number, number?]): this;
    setOptions(options: object): this;
    redraw(): this;
  }

  export function heatLayer(
    latlngs: Array<[number, number, number?]>,
    options?: object,
  ): HeatLayer;
}