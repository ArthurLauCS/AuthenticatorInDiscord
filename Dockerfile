FROM python:3.14-slim
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
WORKDIR /app
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt \
    && useradd --uid 10001 --create-home authbot \
    && mkdir -p /app/data && chown authbot:authbot /app/data
COPY authbot ./authbot
COPY THIRD_PARTY_NOTICES.md ./
COPY third_party ./third_party
USER 10001:10001
CMD ["python", "-m", "authbot", "run"]
