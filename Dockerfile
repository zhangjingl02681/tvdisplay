FROM python:3.12-slim

WORKDIR /app
COPY server/pyproject.toml /app/pyproject.toml
COPY server/tvdisplay /app/tvdisplay
RUN pip install --no-cache-dir .

ENV HOST=::
ENV PORT=8080
ENV DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD python -m tvdisplay.healthcheck

CMD ["python", "-m", "tvdisplay"]
